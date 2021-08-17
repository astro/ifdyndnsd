use std::net::{IpAddr, Ipv4Addr, SocketAddr};
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

pub struct DnsServer {
    client: AsyncClient,
}

impl DnsServer {
    pub async fn new(addr: IpAddr, key: &crate::config::TsigKey) -> Self {
        let alg = match key.alg.as_str() {
            "hmac-sha224" => tsig::Algorithm::HmacSha224,
            "hmac-sha256" => tsig::Algorithm::HmacSha256,
            "hmac-sha384" => tsig::Algorithm::HmacSha384,
            "hmac-sha512" => tsig::Algorithm::HmacSha512,
            _ => panic!("Invalid TSig algorithm: {}", key.alg)
        };
        let signer = Signer {
            key: tsig::Key::new(
                Name::from_str(&key.name).unwrap(),
                alg,
                key.secret.bytes().collect::<Vec<u8>>()
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

        let mut this = DnsServer { client };
        this.query("spaceboyz.net", RecordType::AAAA).await.unwrap();
        this
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

    pub async fn update(&mut self, name: &str, addr: IpAddr) -> Result<(), String> {
        let rdata = match addr {
            IpAddr::V4(addr) => RData::A(addr),
            IpAddr::V6(addr) => RData::AAAA(addr),
        };
        let rec = Record::from_rdata(Name::from_str(name)?, 0, rdata);
        let origin = Name::from_str("dyn.spaceboyz.net")?;
        // let query = self.client.delete_rrset(rec, origin);
        let query = self.client.append(rec, origin, false);
        let response = query.await
            .map_err(|e| format!("{}", e))?;

        if response.response_code() != ResponseCode::NoError {
            Err(format!("Response code: {}", response.response_code()))?;
        }
        println!("res: {:?}", response);
        Ok(())
    }
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
