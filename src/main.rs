mod config;
mod ifaces;
mod dns;

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::rc::Rc;
use std::str::FromStr;
use std::time::Duration;
use cidr::IpCidr;
use tokio::time::timeout;

struct RecordState {
    key: Rc<config::TsigKey>,
    addr: Option<IpAddr>,
    scope: IpCidr,
    dirty: bool,
}

impl RecordState {
    fn new(iface: config::Interface, key: Rc<config::TsigKey>, default_scope: &str) -> Self {
        RecordState {
            key,
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
    println!("{:?}", config);

    let keys = config.keys.into_iter()
        .map(|(name, key)| (name, Rc::new(key)))
        .collect::<HashMap<_, _>>();
    let mut iface_states = HashMap::<String, Vec<RecordState>>::new();
    for a in config.a.into_iter() {
        let key = keys.get(&a.key).unwrap();
        iface_states.entry(a.interface.clone())
            .or_insert(vec![])
            .push(RecordState::new(a, key.clone(), "0.0.0.0/0"));
    }
    for aaaa in config.aaaa.into_iter() {
        let key = keys.get(&aaaa.key).unwrap();
        iface_states.entry(aaaa.interface.clone())
            .or_insert(vec![])
            .push(RecordState::new(aaaa, key.clone(), "2000::/3"));
    }

    let mut addr_updates = ifaces::start();

    // dns::query().await?;
    // dns::update().await?;

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
                println!("IDLE_TIMEOUT");

                'send_update:
                for (iface, states) in &mut iface_states {
                    for state in states.iter_mut() {
                        if state.dirty {
                            println!("Update {}: {}", iface, state.addr.unwrap());
                            state.dirty = false;
                            break 'send_update;
                        }
                    }
                }
            }
        }
    }
}
