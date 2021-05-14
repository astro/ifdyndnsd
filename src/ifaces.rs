use std::collections::{HashMap, hash_map::Entry, HashSet};
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

pub async fn start() {
    loop {
        match run().await {
            Ok(()) => println!("nfnetlink: restarting listener"),
            Err(e) => println!("nfnetlink error: {}", e),
        }
    }
}

async fn run() -> Result<(), String> {
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
    let mut interface_addresses = HashMap::new();
    let links = handle
        .link()
        .get()
        .execute();
    links.try_for_each(|m| {
        let index = m.header.index;
        if let Some(name) = link_message_name(&m) {
            println!("{} - {:x?}", name, m);
            interface_names.insert(index, name.to_owned());
        }
        ok(())
    }).await.map_err(|e| format!("{:x?}", e))?;

    handle.address().get().execute().try_for_each(|m| {
        if let Some(name) = interface_names.get(&m.header.index) {
            let mut addr = None;
            let mut flags = None;

            for nla in &m.nlas {
                match nla {
                    AddrNla::Address(a) => addr = Some(a.clone()),
                    AddrNla::Flags(f) => flags = Some(f),
                    _ => {}
                }
            }
            match (addr, flags) {
                (Some(addr), Some(flags)) => {
                    let temp = flags & IFA_F_TEMPORARY != 0;
                    // println!("{}{} {:x?}", name, if temp { " (temp)" } else { "" }, addr);
                    if !temp {
                        let addrs = interface_addresses.entry(m.header.index)
                            .or_insert(HashSet::new());
                        addrs.insert(addr);
                    }
                }
                _ => {}
            }
        }
        
        ok(())
    }).await.map_err(|e| format!("{:x?}", e))?;
    
    while let Some((message, _)) = messages.next().await {
        match message.payload {
            NetlinkPayload::InnerMessage(RtnlMessage::NewLink(m)) => {
                let index = m.header.index;
                if let Some(name) = link_message_name(&m) {
                    interface_names.insert(index, name.to_owned());
                }
            }
            NetlinkPayload::InnerMessage(RtnlMessage::DelLink(m)) => {
                interface_names.remove(&m.header.index);
            }
            NetlinkPayload::InnerMessage(RtnlMessage::NewAddress(m)) => {
                if let Some(name) = interface_names.get(&m.header.index) {
                    let mut addr = None;
                    let mut flags = None;

                    for nla in &m.nlas {
                        match nla {
                            AddrNla::Address(a) => addr = Some(a.clone()),
                            AddrNla::Flags(f) => flags = Some(f),
                            _ => {}
                        }
                    }
                    match (addr, flags) {
                        (Some(addr), Some(flags)) => {
                            let temp = flags & IFA_F_TEMPORARY != 0;
                            if !temp {
                                // interface_addresses.(m.header.index, addr);
                            }
                        }
                        _ => {}
                    }
                } else {
                    println!("No such link with index={}", m.header.index);
                }
            }
            _ =>
                println!("Other - {:x?}", message.payload),
        }
    }
    Ok(())
}

fn link_message_name(m: &LinkMessage) -> Option<&String> {
    m.nlas.iter().filter_map(|nla| {
        match nla {
            LinkNla::IfName(name) => Some(name),
            _ => None,
        }
    }).next()
}
