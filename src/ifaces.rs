use futures::{
    future::ok,
    stream::{StreamExt, TryStreamExt},
};
use log::{debug, error, trace};
use netlink_packet_core::NetlinkPayload;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use netlink_packet_route::{
    constants::IFA_F_TEMPORARY, rtnl::address::nlas::Nla as AddrNla,
    rtnl::link::nlas::Nla as LinkNla, AddressMessage, LinkMessage, RtnlMessage,
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
                Err(e) => error!("nfnetlink error: {}", e),
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
        trace!("netlink message: {:?}", message);
        match message.payload {
            NetlinkPayload::InnerMessage(RtnlMessage::NewLink(m)) => {
                let index = m.header.index;
                if let Some(name) = link_message_name(&m) {
                    interface_names.insert(index, name.to_string());
                }
            }
            NetlinkPayload::InnerMessage(RtnlMessage::DelLink(m)) => {
                interface_names.remove(&m.header.index);
            }
            NetlinkPayload::InnerMessage(RtnlMessage::NewAddress(m)) => {
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
    let mut addr_buf = None;
    let mut local_buf = None;
    let mut flags = None;

    for nla in &m.nlas {
        match nla {
            AddrNla::Address(a) => addr_buf = Some(a.clone()),
            AddrNla::Local(a) => local_buf = Some(a.clone()),
            AddrNla::Flags(f) => flags = Some(f),
            _ => {}
        }
    }
    match (
        addr_buf.and_then(buf_to_addr),
        local_buf.and_then(buf_to_addr),
        flags,
    ) {
        (_, _, Some(flags)) if flags & IFA_F_TEMPORARY != 0 =>
        // ignore temporary addresses
        {
            None
        }
        (_, Some(addr), _) =>
        // prefer local address in case of pointopoint ifaces
        {
            Some(addr)
        }
        (Some(addr), _, _) => Some(addr),
        (_, _, _) => None,
    }
}

fn buf_to_addr(addr: Vec<u8>) -> Option<IpAddr> {
    match addr.len() {
        4 => <[u8; 4]>::try_from(addr)
            .map(Ipv4Addr::from)
            .map(IpAddr::V4)
            .ok(),
        16 => <[u8; 16]>::try_from(addr)
            .map(Ipv6Addr::from)
            .map(IpAddr::V6)
            .ok(),
        _ => None,
    }
}

fn link_message_name(m: &LinkMessage) -> Option<&String> {
    m.nlas.iter().find_map(|nla| match nla {
        LinkNla::IfName(name) => Some(name),
        _ => None,
    })
}
