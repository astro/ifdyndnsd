use ifdyndnsd::{config, dns, ifaces, AddressFamily, RecordState};
use log::{debug, error, trace};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};
use tokio::time::timeout;

#[tokio::main]
async fn main() -> Result<(), String> {
    const IDLE_TIMEOUT: Duration = Duration::from_secs(1);
    const NEVER_TIMEOUT: Duration = Duration::from_secs(365 * 86400);

    env_logger::init();

    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 2 {
        error!("Usage: {} <config.toml>", args[0]);
        std::process::exit(1);
    }
    let config_file = &args[1];
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
            Rc::new(RefCell::new(dns::Server::new(key.server, key).await)),
        );
    }
    let mut iface_states = HashMap::<String, Vec<RecordState>>::new();
    for a in config.a.unwrap_or_default() {
        let server = servers.get(&a.key).unwrap();
        iface_states
            .entry(a.interface.clone())
            .or_insert_with(Vec::new)
            .push(RecordState::new(a, server.clone(), AddressFamily::IPv4));
    }
    for aaaa in config.aaaa.unwrap_or_default() {
        let server = servers.get(&aaaa.key).unwrap();
        iface_states
            .entry(aaaa.interface.clone())
            .or_insert_with(Vec::new)
            .push(RecordState::new(aaaa, server.clone(), AddressFamily::IPv6));
    }

    let mut interval = NEVER_TIMEOUT;

    let mut addr_updates = ifaces::start();

    loop {
        trace!("recv for {:?}", interval);
        match timeout(interval, addr_updates.recv()).await {
            Ok(Some((iface, addr))) => {
                trace!("interface {}: address {}", iface, addr);
                if let Some(states) = iface_states.get_mut(&iface) {
                    for record_state in states.iter_mut() {
                        if record_state.set_address(addr) {
                            debug!("interface {}: new address {}", iface, addr);
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
