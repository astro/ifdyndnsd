use log::info;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use trust_dns_client::client::{AsyncClient, ClientHandle, Signer};
use trust_dns_client::op::{Message, MessageFinalizer, MessageVerifier, ResponseCode};
use trust_dns_client::proto::error::ProtoResult;
use trust_dns_client::rr::dnssec::tsig::TSigner;
use trust_dns_client::rr::rdata::tsig::TsigAlgorithm;
use trust_dns_client::rr::{DNSClass, Name, RData, Record, RecordType};
use trust_dns_client::udp::UdpClientStream;

//mod tsig;

pub struct Server {
    client: AsyncClient,
}

impl Server {
    /// # Panics
    ///
    /// Will panic if
    ///
    /// - Configuration parameter `key.alg` is non-ascii or doesn't match a valid algorithm.
    /// - Configuration parameter `key.name` could not be parsed into a UTF-8 string.
    /// - Establishing a connection to the DNS endpoint failed.
    ///
    pub async fn new(addr: IpAddr, key: &crate::config::TsigKey) -> Self {
        //let alg = tsig::Algorithm::from_name(&Name::from_ascii(&key.alg).unwrap()).unwrap();
        // TODO: Why from DNS name?
        let alg = TsigAlgorithm::from_name(Name::from_str(&key.alg).unwrap());

        //let signer = Signer {
        //    key: tsig::Key::new(Name::from_str(&key.name).unwrap(), alg, key.get_secret()),
        //};

        let signer = TSigner::new(
            key.get_secret(),
            alg,
            Name::from_str(&key.name).unwrap(),
            300,
        )
        .unwrap();

        let stream = UdpClientStream::<UdpSocket, TSigner>::with_timeout_and_signer(
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
    /// # Errors
    ///
    /// Will return `Err` if
    ///
    /// - `name` could not be parsed into a UTF-8 string.
    /// - The DNS query failed.
    ///
    pub async fn query(
        &mut self,
        name: &str,
        record_type: RecordType,
    ) -> Result<Vec<IpAddr>, String> {
        let query = self
            .client
            .query(Name::from_str(name)?, DNSClass::IN, record_type);
        let response = query.await.map_err(|e| format!("{e}"))?;

        let result = response
            .answers()
            .iter()
            .filter_map(|answer| match answer.data() {
                Some(RData::A(addr)) => Some((*addr).into()),
                Some(RData::AAAA(addr)) => Some((*addr).into()),
                _ => None,
            })
            .collect::<Vec<_>>();

        Ok(result)
    }
    /// # Errors
    ///
    /// Will return `Err` in case
    ///
    /// - `name` can not be parsed into a UTF-8 string.
    /// - deletion of resource record set failed.
    /// - appending the new record failed.
    ///

    pub async fn update(
        &mut self,
        name: &str,
        addr: IpAddr,
        zone: Option<&str>,
        ttl: u32,
    ) -> Result<(), String> {
        let rdata = match addr {
            IpAddr::V4(addr) => RData::A(addr),
            IpAddr::V6(addr) => RData::AAAA(addr),
        };
        let name = Name::from_str(name)?;

        // This is introduced to deal with legacy configurations without a `zone` set.
        let zone = match zone {
            Some(zone) => Name::from_str(zone)?,
            None => name.base_name(),
        };
        let rec = Record::from_rdata(name.clone(), ttl, rdata);
        let query = self.client.delete_rrset(rec.clone(), zone.clone());
        let response = query.await.map_err(|e| format!("{e}"))?;

        if response.response_code() != ResponseCode::NoError {
            return Err(format!("Response code: {}", response.response_code()));
        }
        let query = self.client.append(rec, zone, false);
        info!("DNS update: {} {}", name, addr);
        let response = query.await.map_err(|e| format!("{e}"))?;

        if response.response_code() != ResponseCode::NoError {
            return Err(format!("Response code: {}", response.response_code()));
        }
        Ok(())
    }
}

//struct Signer {
//    key: tsig::Key,
//}
//
//impl MessageFinalizer for Signer {
//    fn finalize_message(
//        &self,
//        message: &Message,
//        current_time: u32,
//    ) -> ProtoResult<(Vec<Record>, Option<MessageVerifier>)> {
//        let record = tsig::create_signature(message, current_time.into(), &self.key).unwrap();
//        Ok((vec![record], None))
//    }
//}
