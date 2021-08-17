mod config;
mod ifaces;
mod dns;

use std::cell::RefCell;
use std::collections::HashMap;
use std::net::IpAddr;
use std::rc::Rc;
use std::str::FromStr;
use std::time::Duration;
use cidr::IpCidr;
use tokio::time::timeout;
use trust_dns_client::rr::RecordType;

struct RecordState {
    server: Rc<RefCell<dns::DnsServer>>,
    hostname: String,

    addr: Option<IpAddr>,
    scope: IpCidr,
    dirty: bool,
}

impl RecordState {
    fn new(iface: config::Interface, server: Rc<RefCell<dns::DnsServer>>, default_scope: &str) -> Self {
        RecordState {
            server,
            hostname: iface.name.clone(),

            addr: None,
            scope: IpCidr::from_str(match &iface.scope {
                Some(s) => s,
                None => default_scope,
            }).unwrap(),
            dirty: false,
        }
    }

    fn set_address(&mut self, addr: IpAddr) {
        // check scope
        if ! self.scope.contains(&addr) {
            return;
        }

        if self.addr == Some(addr) {
            // No change
            return;
        }

        self.addr = Some(addr);
        self.dirty = true;
    }

    async fn update(&mut self) {
        self.dirty = false;

        let record_type;
        match self.addr {
            None => return,
            Some(IpAddr::V4(_)) =>
                record_type = RecordType::A,
            Some(IpAddr::V6(_)) =>
                record_type = RecordType::AAAA,
        };

        let mut server = self.server.borrow_mut();
        match server.query(&self.hostname, record_type).await {
            Ok(addr) if Some(addr) == self.addr => {
                println!("No address change for {}: {}", self.hostname, addr);
                return;
            }
            Ok(addr) => {
                println!("Outdated address for {}: {}", self.hostname, addr);
            }
            Err(e) => {
                println!("Error querying for {} {}: {}", record_type, self.hostname, e);
            }
        }

        server.update(&self.hostname, self.addr.unwrap()).await
            .unwrap_or_else(|e| println!("{}", e));
    }
}

const IDLE_TIMEOUT: u64 = 1000;

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
    for a in config.a.into_iter() {
        let server = servers.get(&a.key).unwrap();
        iface_states.entry(a.interface.clone())
            .or_insert(vec![])
            .push(RecordState::new(a, server.clone(), "0.0.0.0/0"));
    }
    for aaaa in config.aaaa.into_iter() {
        let server = servers.get(&aaaa.key).unwrap();
        iface_states.entry(aaaa.interface.clone())
            .or_insert(vec![])
            .push(RecordState::new(aaaa, server.clone(), "2000::/3"));
    }

    let mut addr_updates = ifaces::start();

    loop {
        match timeout(Duration::from_millis(IDLE_TIMEOUT), addr_updates.recv()).await {
            Ok(Some((iface, addr))) => {
                println!("{}: {}", iface, addr);
                iface_states.get_mut(&iface)
                    .map(|states| {
                        for record_state in states.iter_mut() {
                            record_state.set_address(addr);
                        }
                    });
            }
            Ok(None) =>
                return Ok(()),
            Err(_) => {
                /* IDLE_TIMEOUT reached */

                'send_update:
                for (_iface, states) in &mut iface_states {
                    for state in states.iter_mut() {
                        if state.dirty {
                            state.update().await;
                            break 'send_update;
                        }
                    }
                }
            }
        }
    }
}
