pub mod config;
pub mod dns;
pub mod ifaces;

use cidr::IpCidr;
use hickory_client::rr::RecordType;
use log::{debug, error, info, trace, warn};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::rc::Rc;
use std::str::FromStr;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::time::timeout;

pub const RETRY_INTERVAL: u64 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressFamily {
    IPv4,
    IPv6,
}

pub struct RecordState {
    server: Rc<Mutex<dns::Server>>,
    name: Option<Rc<String>>,
    neighbors: Rc<HashMap<String, Ipv6Addr>>,

    addr: Option<IpAddr>,
    ttl: u32,
    zone: Option<Rc<String>>,
    scope: IpCidr,
    dirty: bool,
    update_tried: Option<Instant>,
}

impl RecordState {
    /// # Panics
    ///
    /// Will panic if the `scope` setting could not be parsed as a
    /// Classless Inter-Domain Routing (CIDR) address.
    pub fn new(
        update_task: config::UpdateTask,
        server: Rc<Mutex<dns::Server>>,
        af: AddressFamily,
    ) -> Self {
        let scope = IpCidr::from_str(update_task.scope.as_deref().unwrap_or(match af {
            crate::AddressFamily::IPv4 => "0.0.0.0/0",
            AddressFamily::IPv6 => "2000::/3",
        }))
        .unwrap();
        match af {
            AddressFamily::IPv4 if update_task.neighbors.is_some() => {
                panic!("neighbors are not supported on IPv4");
            }
            AddressFamily::IPv4 if scope.is_ipv4() => {}
            AddressFamily::IPv6 if scope.is_ipv6() => {}
            _ => panic!("scope {} doesn't match address family {:?}", scope, af),
        }

        let zone = if let Some(zone) = update_task.zone {
            Some(Rc::new(zone))
        } else {
            warn!("Your configuration misses the `zone` parameter. This field will be mandatory in a future release.");
            None
        };
        //let zone = match update_task.zone {
        //    Some(zone) => Some(Rc::new(zone)),
        //    None => {
        //        warn!("Your configuration misses the `zone` parameter. This field will be mandatory in a future release.");
        //        None
        //    }
        //};

        let name = update_task.name.map(|name| Rc::new(name.clone()));

        RecordState {
            server,
            name,
            neighbors: Rc::new(update_task.neighbors.unwrap_or_default()),

            addr: None,
            ttl: update_task.ttl.unwrap_or(0),
            zone,
            scope,
            dirty: false,
            update_tried: None,
        }
    }

    pub fn set_address(&mut self, addr: IpAddr) -> bool {
        // check scope
        if !self.scope.contains(&addr) {
            return false;
        }

        if self.addr == Some(addr) {
            // No change
            return false;
        }

        self.addr = Some(addr);
        self.dirty = true;
        self.update_tried = None;
        true
    }

    #[must_use]
    pub fn can_update(&self) -> bool {
        match self.update_tried {
            // nothing to do
            _ if !self.dirty => false,

            // new
            None => true,

            // retry if RETRY_INTERVAL elapsed
            Some(update_tried) => {
                Instant::now() >= update_tried + Duration::from_secs(RETRY_INTERVAL)
            }
        }
    }

    #[must_use]
    pub fn next_timeout(&self) -> Option<Instant> {
        if self.dirty {
            self.update_tried
                .map(|update_tried| update_tried + Duration::from_secs(RETRY_INTERVAL))
                .or_else(|| Some(Instant::now()))
        } else {
            None
        }
    }
    /// # Panics
    ///
    /// Will panic if the config (`[[aaaa]]` or `[[a]]`) misses an `address`.
    pub async fn update(&mut self) {
        self.dirty = false;
        self.update_tried = Some(Instant::now());

        let addr = self.addr.unwrap();
        if let Some(name) = &self.name.clone() {
            if let Err(e) = self.update_addr(name, &addr).await {
                error!("Error updating {} to {}: {}", name, self.addr.unwrap(), e);
                // try again later
                self.dirty = true;
                return;
            }
        }

        if let Some(IpAddr::V6(addr)) = self.addr {
            for (neighbor_name, neighbor_addr) in &*self.neighbors.clone() {
                let net_segs = addr.segments();
                let host_segs = neighbor_addr.segments();
                let addr = Ipv6Addr::new(
                    net_segs[0],
                    net_segs[1],
                    net_segs[2],
                    net_segs[3],
                    host_segs[4],
                    host_segs[5],
                    host_segs[6],
                    host_segs[7],
                )
                .into();

                if let Err(e) = self.update_addr(neighbor_name, &addr).await {
                    error!("Error updating neighbor {neighbor_addr} to {addr}: {e}");
                }
            }
        }
    }

    async fn update_addr(&mut self, name: &str, addr: &IpAddr) -> Result<(), String> {
        let record_type = match addr {
            IpAddr::V4(_) => RecordType::A,
            IpAddr::V6(_) => RecordType::AAAA,
        };

        let mut server = self.server.lock().await;
        match server.query(name, record_type).await {
            Ok(addrs) if addrs.len() == 1 && addrs[0] == *addr => {
                info!("No address change for {name} ({addr} == {addrs:?})");
                return Ok(());
            }
            Ok(addrs) => {
                info!("Outdated address for {name}: {addrs:?}");
            }
            Err(e) => {
                error!("Error querying for {record_type} {name}: {e}");
            }
        }

        let zone = self.zone.as_ref().map(|zone| zone.as_str());

        server.update(name, *addr, zone, self.ttl).await
    }
}
/// # Errors
///
/// Will return `Err` if `config_file` does not exist or the user does not have
/// permission to read it.
///
/// # Panics
///
/// Will panic if the config (`[[aaaa]]` or `[[a]]`) misses a key.
pub async fn run(config_file: &str) -> Result<(), String> {
    const IDLE_TIMEOUT: Duration = Duration::from_secs(1);
    const NEVER_TIMEOUT: Duration = Duration::from_secs(365 * 86400);

    let config = config::load(config_file)?;

    let keys = config
        .keys
        .into_iter()
        .map(|(name, key)| (name, Rc::new(key)))
        .collect::<HashMap<_, _>>();
    let mut servers = HashMap::new();
    for (name, key) in &keys {
        servers.insert(
            name,
            Rc::new(Mutex::new(dns::Server::new(key.server, key).await)),
        );
    }
    let mut iface_states = HashMap::<String, Vec<RecordState>>::new();
    for a in config.a.unwrap_or_default() {
        let server = servers.get(&a.key).unwrap();
        iface_states
            .entry(a.interface.clone())
            .or_default()
            .push(RecordState::new(a, server.clone(), AddressFamily::IPv4));
    }
    for aaaa in config.aaaa.unwrap_or_default() {
        let server = servers.get(&aaaa.key).unwrap();
        iface_states
            .entry(aaaa.interface.clone())
            .or_default()
            .push(RecordState::new(aaaa, server.clone(), AddressFamily::IPv6));
    }

    let mut interval = NEVER_TIMEOUT;

    let mut addr_updates = ifaces::start();

    loop {
        trace!("recv for {interval:?}");
        match timeout(interval, addr_updates.recv()).await {
            Ok(Some((iface, addr))) => {
                trace!("interface {iface}: address {addr}");
                if let Some(states) = iface_states.get_mut(&iface) {
                    for record_state in &mut *states {
                        if record_state.set_address(addr) {
                            debug!("interface {iface}: new address {addr}");
                            interval = IDLE_TIMEOUT;
                        }
                    }
                }
            }
            Ok(None) => {
                error!("netlink disconnect");
                return Err("finished".to_string());
            }
            Err(_) => {
                /* IDLE_TIMEOUT reached */
                interval = NEVER_TIMEOUT;
                debug!("IDLE_TIMEOUT");

                'send_update: for states in iface_states.values_mut() {
                    for state in &mut *states {
                        if state.can_update() {
                            state.update().await;
                            break 'send_update;
                        }
                    }
                }

                /* if NEVER_TIMEOUT was set, find a smaller timeout to retry an update */
                for states in iface_states.values() {
                    for state in states {
                        if let Some(state_timeout) = state.next_timeout() {
                            let now = Instant::now();
                            if state_timeout <= now {
                                interval = Duration::from_secs(0);
                            } else {
                                let state_interval = now - state_timeout;
                                if state_interval < interval {
                                    interval = state_interval;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
