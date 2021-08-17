use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::net::UdpSocket;
use trust_dns_client::udp::UdpClientStream;
use trust_dns_client::client::{AsyncClient, ClientHandle};
use trust_dns_client::rr::{DNSClass, Name, RData, Record, RecordType};
use trust_dns_client::op::{Message, MessageFinalizer, MessageVerifier, ResponseCode};
use trust_dns_client::proto::error::ProtoResult;

mod tsig;

pub struct DnsServer {
    client: AsyncClient,
}

impl DnsServer {
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

        DnsServer { client }
    }

    pub async fn query(&mut self, name: &str, record_type: RecordType) -> Result<IpAddr, String> {
        let query = self.client.query(Name::from_str(name)?, DNSClass::IN, record_type);
        let response = query.await
            .map_err(|e| format!("{}", e))?;

        for answer in response.answers() {
            match answer.rdata() {
                RData::A(addr) => return Ok(addr.clone().into()),
                RData::AAAA(addr) => return Ok(addr.clone().into()),
                _ => {}
            }
        }

        Err(format!("No record"))
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
            Err(format!("Response code: {}", response.response_code()))?;
        }
        let query = self.client.append(rec, origin, false);
        println!("DNS update: {} {}", hostname, addr);
        let response = query.await
            .map_err(|e| format!("{}", e))?;

        if response.response_code() != ResponseCode::NoError {
            Err(format!("Response code: {}", response.response_code()))?;
        }
        Ok(())
    }
}

struct Signer {
    key: tsig::Key,
}

impl MessageFinalizer for Signer {
    fn finalize_message(&self, message: &Message, _current_time: u32) -> ProtoResult<(Vec<Record>, Option<MessageVerifier>)> {
        let unix_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
        let record = tsig::create_signature(&message, unix_time.as_secs(), &self.key).unwrap();
        Ok((vec![record], None))
    }
    
}
