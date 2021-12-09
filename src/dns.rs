use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use trust_dns_client::udp::UdpClientStream;
use trust_dns_client::client::{AsyncClient, ClientHandle};
use trust_dns_client::rr::{DNSClass, Name, RData, Record, RecordType};
use trust_dns_client::op::{Message, MessageFinalizer, MessageVerifier, ResponseCode};
use trust_dns_client::proto::error::ProtoResult;

mod tsig;

pub struct Server {
    client: AsyncClient,
}

impl Server {
    pub async fn new(addr: IpAddr, key: &crate::config::TsigKey) -> Self {
        let alg = tsig::Algorithm::from_name(
            &Name::from_ascii(&key.alg).unwrap()
        ).unwrap();
        let signer = Signer {
            key: tsig::Key::new(
                Name::from_str(&key.name).unwrap(),
                alg,
                key.get_secret()
            ),
        };

        let stream = UdpClientStream::<UdpSocket, Signer>::with_timeout_and_signer(
            (addr, 53).into(),
            Duration::from_secs(3),
            Some(Arc::new(signer)),
        );
        let client = AsyncClient::connect(stream);
        let (mut client, bg) = client.await.unwrap();
        client.disable_edns();

        tokio::spawn(bg);

        Server { client }
    }

    pub async fn query(&mut self, name: &str, record_type: RecordType) -> Result<Vec<IpAddr>, String> {
        let query = self.client.query(Name::from_str(name)?, DNSClass::IN, record_type);
        let response = query.await
            .map_err(|e| format!("{}", e))?;

        let result = response.answers()
            .iter()
            .filter_map(|answer| match answer.rdata() {
                RData::A(addr) => Some((*addr).into()),
                RData::AAAA(addr) => Some((*addr).into()),
                _ => None,
            })
            .collect::<Vec<_>>();

        Ok(result)
    }

    pub async fn update(&mut self, hostname: &str, addr: IpAddr) -> Result<(), String> {
        let rdata = match addr {
            IpAddr::V4(addr) => RData::A(addr),
            IpAddr::V6(addr) => RData::AAAA(addr),
        };
        let name = Name::from_str(hostname)?;
        let origin = name.base_name();
        let rec = Record::from_rdata(name, 0, rdata);
        let query = self.client.delete_rrset(rec.clone(), origin.clone());
        let response = query.await
            .map_err(|e| format!("{}", e))?;

        if response.response_code() != ResponseCode::NoError {
            return Err(format!("Response code: {}", response.response_code()));
        }
        let query = self.client.append(rec, origin, false);
        println!("DNS update: {} {}", hostname, addr);
        let response = query.await
            .map_err(|e| format!("{}", e))?;

        if response.response_code() != ResponseCode::NoError {
            return Err(format!("Response code: {}", response.response_code()));
        }
        Ok(())
    }
}

struct Signer {
    key: tsig::Key,
}

impl MessageFinalizer for Signer {
    fn finalize_message(&self, message: &Message, current_time: u32) -> ProtoResult<(Vec<Record>, Option<MessageVerifier>)> {
        let record = tsig::create_signature(message, current_time.into(), &self.key).unwrap();
        Ok((vec![record], None))
    }
    
}
