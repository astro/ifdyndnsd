use std::collections::HashMap;
use std::convert::TryFrom;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use futures::{
    future::ok,
    stream::{StreamExt, TryStreamExt},
};

use rtnetlink::{
    constants::{RTMGRP_IPV4_IFADDR, RTMGRP_IPV6_IFADDR, RTMGRP_LINK},
    new_connection,
    sys::SocketAddr,
};
use netlink_packet_route::{
    NetlinkPayload, LinkMessage, RtnlMessage,
    constants::IFA_F_TEMPORARY,
    rtnl::address::nlas::Nla as AddrNla,
    rtnl::link::nlas::Nla as LinkNla,
};
use tokio::{
    task::spawn,
    sync::mpsc::{channel, Receiver, Sender},
};

pub fn start() -> Receiver<(String, IpAddr)> {
    let (mut tx, rx) = channel(1);

    spawn(async move {
        loop {
            match run(&mut tx).await {
                Ok(()) => eprintln!("nfnetlink: restarting listener"),
                Err(e) => eprintln!("nfnetlink error: {}", e),
            }
        }
    });
    rx
}

async fn run(tx: &mut Sender<(String, IpAddr)>) -> Result<(), String> {
    // Open the netlink socket
    let (mut connection, handle, mut messages) = new_connection()
        .map_err(|e| format!("{}", e))?;

    // These flags specify what kinds of broadcast messages we want to listen for.
    let mgroup_flags = RTMGRP_LINK | RTMGRP_IPV4_IFADDR | RTMGRP_IPV6_IFADDR;

    // A netlink socket address is created with said flags.
    let addr = SocketAddr::new(0, mgroup_flags);
    // Said address is bound so new conenctions and thus new message broadcasts can be received.
    connection.socket_mut().bind(&addr).expect("failed to bind");
    tokio::spawn(connection);

    let mut interface_names = HashMap::new();
    let links = handle
        .link()
        .get()
        .execute();
    links.try_for_each(|m| {
        let index = m.header.index;
        if let Some(name) = link_message_name(&m) {
            interface_names.insert(index, name.to_string());
        }
        ok(())
    }).await.map_err(|e| format!("{:x?}", e))?;

    let mut initial = vec![];
    handle.address().get().execute().try_for_each(|m| {
        if let Some(name) = interface_names.get(&m.header.index) {
            let mut addr = None;
            let mut flags = None;

            for nla in &m.nlas {
                match nla {
                    AddrNla::Address(a) =>
                        addr = Some(a.clone()),
                    AddrNla::Flags(f) =>
                        flags = Some(f),
                    _ => {}
                }
            }
            if let (Some(addr), Some(flags)) = (addr, flags) {
                let temp = flags & IFA_F_TEMPORARY != 0;
                if !temp {
                    if let Some(addr) = buf_to_addr(addr) {
                        initial.push((name.clone(), addr));
                    }
                }
            }
        }
        
        ok(())
    }).await.map_err(|e| format!("{:x?}", e))?;

    for value in initial {
        tx.send(value).await.unwrap();
    }
    
    while let Some((message, _)) = messages.next().await {
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
                    let mut addr_buf = None;
                    let mut flags = None;

                    for nla in &m.nlas {
                        match nla {
                            AddrNla::Address(a) =>
                                addr_buf = Some(a.clone()),
                            AddrNla::Flags(f) =>
                                flags = Some(f),
                            _ => {}
                        }
                    }
                    if let (Some(addr_buf), Some(flags)) = (addr_buf, flags) {
                        let temp = flags & IFA_F_TEMPORARY != 0;
                        if !temp {
                            if let Some(addr) = buf_to_addr(addr_buf) {
                                tx.send((name.clone(), addr)).await.unwrap();
                            }
                        }
                    }
                } else {
                    eprintln!("No such link with index={}", m.header.index);
                }
            }
            _ => {
                // println!("Other - {:x?}", message.payload);
            }
        }
    }
    Ok(())
}

fn buf_to_addr(addr: Vec<u8>) -> Option<IpAddr> {
    match addr.len() {
        4 =>
            <[u8; 4]>::try_from(addr)
            .map(Ipv4Addr::from)
            .map(IpAddr::V4)
            .ok(),
        16 =>
            <[u8; 16]>::try_from(addr)
            .map(Ipv6Addr::from)
            .map(IpAddr::V6)
            .ok(),
        _ =>
            None
    }
}

fn link_message_name(m: &LinkMessage) -> Option<&String> {
    m.nlas.iter().find_map(|nla| {
        match nla {
            LinkNla::IfName(name) => Some(name),
            _ => None,
        }
    })
}
