use std::net::{Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::net::{TcpSocket, UdpSocket};
use trust_dns_client::tcp::TcpClientStream;
use trust_dns_client::udp::UdpClientStream;
use trust_dns_client::client::{Client, AsyncClient, ClientHandle};
use trust_dns_client::rr::{DNSClass, Name, RData, Record, RecordType};
use trust_dns_client::op::{Message, MessageFinalizer, MessageVerifier, ResponseCode};
use trust_dns_client::rr::dnssec::{Algorithm, KeyPair};
use trust_dns_client::rr::rdata::key::{KeyUsage, KEY};
use trust_dns_client::proto::error::ProtoResult;

mod tsig;

pub async fn query() -> Result<(), String> {
    let stream = UdpClientStream::<UdpSocket>::new(([172,22,24,4], 53).into());
    let client = AsyncClient::connect(stream);
    let (mut client, bg) = client.await?;

    tokio::spawn(bg);

    let query = client.query(Name::from_str("astro.dyn.spaceboyz.net.")?, DNSClass::IN, RecordType::A);
    let response = query.await
        .map_err(|e| format!("{}", e))?;

    // if let &RData::A(addr) = response.answers()[0].rdata() {
    //     println!("a: {}", addr);
    // }
    Ok(())
}

struct Signer {
    key: tsig::Key,
}

impl MessageFinalizer for Signer {
    fn finalize_message(&self, message: &Message, current_time: u32) -> ProtoResult<(Vec<Record>, Option<MessageVerifier>)> {
        println!("finalize {:?}", message);
        let unix_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
        let record = tsig::create_signature(&message, unix_time.as_secs(), &self.key).unwrap();
        Ok((vec![record], None))
    }
    
}

pub async fn update() -> Result<(), String> {
    let signer = Signer {
        key: tsig::Key::new(
            Name::from_str("astro-test.dyn.spaceboyz.net").unwrap(),
            tsig::Algorithm::HmacSha256,
            b"secret not as base64".to_vec()
        ),
    };

    let stream = UdpClientStream::<UdpSocket, Signer>::with_timeout_and_signer(
        ([172,22,24,4], 53).into(),
        Duration::from_secs(3),
        Some(Arc::new(signer)),
    );
    let client = AsyncClient::connect(stream);
    let (mut client, bg) = client.await?;
    client.disable_edns();

    tokio::spawn(bg);

    let rec = Record::from_rdata(Name::from_str("test.dyn.spaceboyz.net")?, 0, RData::A(Ipv4Addr::LOCALHOST));
    let origin = Name::from_str("dyn.spaceboyz.net")?;
    let query = client.delete_rrset(rec, origin);
    // let query = client.append(rec, origin, false);
    let response = query.await
        .map_err(|e| format!("{}", e))?;

    if response.response_code() != ResponseCode::NoError {
        Err(format!("Response code: {}", response.response_code()))?;
    }
    println!("res: {:?}", response);
    Ok(())
}
