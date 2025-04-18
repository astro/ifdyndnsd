use futures::{
    future::ok,
    stream::{StreamExt, TryStreamExt},
};
use log::{debug, error, trace};
use netlink_packet_core::NetlinkPayload;
use std::collections::HashMap;
use std::net::IpAddr;

use netlink_packet_route::{
    address::{AddressAttribute, AddressHeaderFlags, AddressMessage},
    link::{LinkAttribute, LinkMessage},
    RouteNetlinkMessage,
};

use netlink_sys::{AsyncSocket, SocketAddr};
use rtnetlink::{
    constants::{RTMGRP_IPV4_IFADDR, RTMGRP_IPV6_IFADDR, RTMGRP_LINK},
    new_connection,
};
use tokio::{
    sync::mpsc::{channel, Receiver, Sender},
    task::spawn,
};

#[must_use]
pub fn start() -> Receiver<(String, IpAddr)> {
    let (mut tx, rx) = channel(1);

    spawn(async move {
        loop {
            match run(&mut tx).await {
                Ok(()) => error!("nfnetlink: restarting listener"),
                Err(e) => error!("nfnetlink error: {e}"),
            }
        }
    });
    rx
}

async fn run(tx: &mut Sender<(String, IpAddr)>) -> Result<(), String> {
    // Open the netlink socket
    let (mut connection, handle, mut messages) = new_connection().map_err(|e| format!("{e}"))?;

    // These flags specify what kinds of broadcast messages we want to listen for.
    let mgroup_flags = RTMGRP_LINK | RTMGRP_IPV4_IFADDR | RTMGRP_IPV6_IFADDR;

    // A netlink socket address is created with said flags.
    let addr = SocketAddr::new(0, mgroup_flags);
    // Said address is bound so new conenctions and thus new message broadcasts can be received.
    connection
        .socket_mut()
        .socket_mut()
        .bind(&addr)
        .expect("failed to bind");
    tokio::spawn(connection);

    let mut interface_names = HashMap::new();
    let links = handle.link().get().execute();
    links
        .try_for_each(|m| {
            let index = m.header.index;
            if let Some(name) = link_message_name(&m) {
                interface_names.insert(index, name.to_string());
            }
            ok(())
        })
        .await
        .map_err(|e| format!("{e:x?}"))?;

    let mut initial = vec![];
    handle
        .address()
        .get()
        .execute()
        .try_for_each(|m| {
            if let Some(name) = interface_names.get(&m.header.index) {
                if let Some(addr) = message_local_addr(&m) {
                    initial.push((name.clone(), addr));
                }
            }

            ok(())
        })
        .await
        .map_err(|e| format!("{e:x?}"))?;

    for value in initial {
        debug!("interface {}: initial address {:?}", value.0, value.1);
        tx.send(value).await.unwrap();
    }

    while let Some((message, _)) = messages.next().await {
        trace!("netlink message: {message:?}");
        match message.payload {
            NetlinkPayload::InnerMessage(RouteNetlinkMessage::NewLink(m)) => {
                let index = m.header.index;
                if let Some(name) = link_message_name(&m) {
                    interface_names.insert(index, name.to_string());
                }
            }
            NetlinkPayload::InnerMessage(RouteNetlinkMessage::DelLink(m)) => {
                interface_names.remove(&m.header.index);
            }
            NetlinkPayload::InnerMessage(RouteNetlinkMessage::NewAddress(m)) => {
                if let Some(name) = interface_names.get(&m.header.index) {
                    if let Some(addr) = message_local_addr(&m) {
                        tx.send((name.clone(), addr)).await.unwrap();
                    }
                } else {
                    error!("No such link with index={}", m.header.index);
                }
            }
            _ => {
                // println!("Other - {:x?}", message.payload);
            }
        }
    }
    Ok(())
}

fn message_local_addr(m: &AddressMessage) -> Option<IpAddr> {
    // Ignore IPv6 temp_addrs
    let is_temporary = m.header.flags.contains(AddressHeaderFlags::Secondary);
    if is_temporary {
        return None;
    }

    // Get the local address for a pointopoint link
    if let Some(local) = m.attributes.iter().find_map(|a| {
        if let AddressAttribute::Local(addr) = a {
            Some(*addr)
        } else {
            None
        }
    }) {
        return Some(local);
    }

    // Get interfaces address
    m.attributes.iter().find_map(|a| {
        if let AddressAttribute::Address(addr) = a {
            Some(*addr)
        } else {
            None
        }
    })
}

fn link_message_name(m: &LinkMessage) -> Option<&String> {
    m.attributes.iter().find_map(|a| {
        if let LinkAttribute::IfName(name) = a {
            Some(name)
        } else {
            None
        }
    })
}
