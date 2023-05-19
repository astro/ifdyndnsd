pub mod config;
pub mod dns;
pub mod ifaces;

use cidr::IpCidr;
use log::{error, info};
use std::cell::RefCell;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::rc::Rc;
use std::str::FromStr;
use std::time::{Duration, Instant};
use trust_dns_client::rr::RecordType;

pub const RETRY_INTERVAL: u64 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressFamily {
    IPv4,
    IPv6,
}

pub struct RecordState {
    server: Rc<RefCell<dns::Server>>,
    hostname: Rc<String>,
    neighbors: Rc<HashMap<String, Ipv6Addr>>,

    addr: Option<IpAddr>,
    scope: IpCidr,
    dirty: bool,
    update_tried: Option<Instant>,
}

impl RecordState {
    pub fn new(
        iface: config::Interface,
        server: Rc<RefCell<dns::Server>>,
        af: AddressFamily,
    ) -> Self {
        let scope = IpCidr::from_str(iface.scope.as_deref().unwrap_or(match af {
            crate::AddressFamily::IPv4 => "0.0.0.0/0",
            AddressFamily::IPv6 => "2000::/3",
        }))
        .unwrap();
        match af {
            AddressFamily::IPv4 if iface.neighbors.is_some() => {
                panic!("neighbors are not supported on IPv4");
            }
            AddressFamily::IPv4 if scope.is_ipv4() => {}
            AddressFamily::IPv6 if scope.is_ipv6() => {}
            _ => panic!("scope {} doesn't match address family {:?}", scope, af),
        }

        RecordState {
            server,
            hostname: Rc::new(iface.name.clone()),
            neighbors: Rc::new(iface.neighbors.unwrap_or_default()),

            addr: None,
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

    pub fn next_timeout(&self) -> Option<Instant> {
        if self.dirty {
            self.update_tried
                .map(|update_tried| update_tried + Duration::from_secs(RETRY_INTERVAL))
                .or_else(|| Some(Instant::now()))
        } else {
            None
        }
    }

    pub async fn update(&mut self) {
        self.dirty = false;
        self.update_tried = Some(Instant::now());

        let addr = self.addr.unwrap();
        if let Err(e) = self.update_addr(&self.hostname.clone(), &addr).await {
            error!(
                "Error updating {} to {}: {}",
                self.hostname,
                self.addr.unwrap(),
                e
            );
            // try again later
            self.dirty = true;
            return;
        }

        if let Some(IpAddr::V6(addr)) = self.addr {
            for (neighbor_name, neighbor_addr) in self.neighbors.clone().iter() {
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
                    error!(
                        "Error updating neighbor {} to {}: {}",
                        neighbor_addr, addr, e
                    );
                }
            }
        }
    }

    async fn update_addr(&mut self, name: &str, addr: &IpAddr) -> Result<(), String> {
        let record_type = match addr {
            IpAddr::V4(_) => RecordType::A,
            IpAddr::V6(_) => RecordType::AAAA,
        };

        let mut server = self.server.borrow_mut();
        match server.query(name, record_type).await {
            Ok(addrs) if addrs.len() == 1 && addrs[0] == *addr => {
                info!("No address change for {} ({} == {:?})", name, addr, addrs);
                return Ok(());
            }
            Ok(addrs) => {
                info!("Outdated address for {}: {:?}", name, addrs);
            }
            Err(e) => {
                error!("Error querying for {} {}: {}", record_type, name, e);
            }
        }

        server.update(name, *addr).await
    }
}
