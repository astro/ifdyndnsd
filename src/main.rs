mod config;
mod ifaces;
mod dns;

use std::cell::RefCell;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::rc::Rc;
use std::str::FromStr;
use std::time::{Duration, Instant};
use cidr::IpCidr;
use tokio::time::timeout;
use trust_dns_client::rr::RecordType;

const RETRY_INTERVAL: u64 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AddressFamily {
    IPv4,
    IPv6,
}

struct RecordState {
    server: Rc<RefCell<dns::DnsServer>>,
    hostname: Rc<String>,
    neighbors: Rc<HashMap<String, Ipv6Addr>>,

    addr: Option<IpAddr>,
    scope: IpCidr,
    dirty: bool,
    update_tried: Option<Instant>,
}

impl RecordState {
    fn new(iface: config::Interface, server: Rc<RefCell<dns::DnsServer>>, af: AddressFamily) -> Self {
        let scope = IpCidr::from_str(
            iface.scope.as_deref()
                .unwrap_or_else(|| match af {
                    AddressFamily::IPv4 => "0.0.0.0/0",
                    AddressFamily::IPv6 => "2000::/3",
                })
        ).unwrap();
        match af {
            AddressFamily::IPv4 if iface.neighbors.is_some() =>
                panic!("neighbors are not supported on IPv4"),
            AddressFamily::IPv4 if scope.is_ipv4() => {}
            AddressFamily::IPv6 if scope.is_ipv6() => {}
            _ => panic!("scope {} doesn't match address family {:?}", scope, af),
        }

        RecordState {
            server,
            hostname: Rc::new(iface.name.clone()),
            neighbors: Rc::new(
                iface.neighbors
                    .unwrap_or_default()
            ),

            addr: None,
            scope,
            dirty: false,
            update_tried: None,
        }
    }

    fn set_address(&mut self, addr: IpAddr) -> bool {
        // check scope
        if ! self.scope.contains(&addr) {
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

    pub fn can_update(&self) -> bool {
        match self.update_tried {
            // nothing to do
            _ if !self.dirty => false,

            // new
            None => true,

            // retry if RETRY_INTERVAL elapsed
            Some(update_tried) => Instant::now() >= update_tried + Duration::from_secs(RETRY_INTERVAL),
        }
    }

    pub fn next_timeout(&self) -> Option<Instant> {
        if self.dirty {
            self.update_tried.map(
                |update_tried| update_tried + Duration::from_secs(RETRY_INTERVAL)
            )
        } else {
            None
        }
    }

    pub async fn update(&mut self) {
        self.dirty = false;
        self.update_tried = Some(Instant::now());

        let addr = self.addr.unwrap();
        if let Err(e) = self.update_addr(&self.hostname.clone(), &addr).await {
            println!("Error updating {} to {}: {}", self.hostname, self.addr.unwrap(), e);
            // try again later
            self.dirty = true;
            return;
        }

        if let Some(IpAddr::V6(addr)) = self.addr {
            for (neighbor_name, neighbor_addr) in self.neighbors.clone().iter() {
                let net_segs = addr.segments();
                let host_segs = neighbor_addr.segments();
                let addr = Ipv6Addr::new(
                    net_segs[0], net_segs[1], net_segs[2], net_segs[3],
                    host_segs[4], host_segs[5], host_segs[6], host_segs[7],
                ).into();

                if let Err(e) = self.update_addr(neighbor_name, &addr).await {
                    println!("Error updating neighbor {} to {}: {}", neighbor_addr, addr, e);
                }
            }
        }
    }

    async fn update_addr(&mut self, name: &str, addr: &IpAddr) -> Result<(), String> {
        let record_type;
        match addr {
            IpAddr::V4(_) =>
                record_type = RecordType::A,
            IpAddr::V6(_) =>
                record_type = RecordType::AAAA,
        };

        let mut server = self.server.borrow_mut();
        match server.query(name, record_type).await {
            Ok(addrs) if addrs.len() == 1 && addrs[0] == *addr => {
                println!("No address change for {}", name);
                return Ok(());
            }
            Ok(addr) => {
                println!("Outdated address for {}: {:?}", name, addr);
            }
            Err(e) => {
                println!("Error querying for {} {}: {}", record_type, name, e);
            }
        }

        server.update(name, *addr).await
    }
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 2 {
        println!("Usage: {} <config.toml>", args[0]);
        std::process::exit(1);
    }
    let config_file = &args[1];
    let config = config::load(config_file)?;

    let keys = config.keys.into_iter()
        .map(|(name, key)| (name, Rc::new(key)))
        .collect::<HashMap<_, _>>();
    let mut servers = HashMap::new();
    for (name, key) in keys.iter() {
        servers.insert(name, Rc::new(RefCell::new(dns::DnsServer::new(key.server, &key).await)));
    }
    let mut iface_states = HashMap::<String, Vec<RecordState>>::new();
    for a in config.a.unwrap_or_default().into_iter() {
        let server = servers.get(&a.key).unwrap();
        iface_states.entry(a.interface.clone())
            .or_insert_with(Vec::new)
            .push(RecordState::new(a, server.clone(), AddressFamily::IPv4));
    }
    for aaaa in config.aaaa.unwrap_or_default().into_iter() {
        let server = servers.get(&aaaa.key).unwrap();
        iface_states.entry(aaaa.interface.clone())
            .or_insert_with(Vec::new)
            .push(RecordState::new(aaaa, server.clone(), AddressFamily::IPv6));
    }

    const IDLE_TIMEOUT: Duration = Duration::from_secs(1);
    const NEVER_TIMEOUT: Duration = Duration::from_secs(365 * 86400);
    let mut interval = NEVER_TIMEOUT;

    let mut addr_updates = ifaces::start();

    loop {
        match timeout(interval, addr_updates.recv()).await {
            Ok(Some((iface, addr))) => {
                if let Some(states) = iface_states.get_mut(&iface) {
                    for record_state in states.iter_mut() {
                        if record_state.set_address(addr) {
                            interval = IDLE_TIMEOUT;
                        }
                    }
                }
            }
            Ok(None) =>
                // netlink closed?
                return Err("finished".to_string()),
            Err(_) => {
                /* IDLE_TIMEOUT reached */
                interval = NEVER_TIMEOUT;

                'send_update:
                for states in iface_states.values_mut() {
                    for state in states.iter_mut() {
                        if state.can_update() {
                            state.update().await;
                            break 'send_update;
                        }
                    }
                }

                /* if NEVER_TIMEOUT was set, find a smaller timeout to retry an update */
                for states in iface_states.values() {
                    for state in states.iter() {
                        if let Some(state_timeout) = state.next_timeout() {
                            let now = Instant::now();
                            if state_timeout <= now {
                                interval = Duration::ZERO;
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
