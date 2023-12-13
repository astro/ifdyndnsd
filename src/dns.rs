use hickory_client::client::{AsyncClient, ClientHandle};
use hickory_client::op::ResponseCode;
use hickory_client::proto::rr::dnssec::tsig::TSigner;
use hickory_client::rr::rdata::{tsig::TsigAlgorithm, A, AAAA};
use hickory_client::rr::{DNSClass, Name, RData, Record, RecordType};
use hickory_client::udp::UdpClientStream;
use log::info;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;

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
        let alg = TsigAlgorithm::from_name(Name::from_str(&key.alg).unwrap());
        let signer = TSigner::new(
            key.get_secret(),
            alg,
            Name::from_str(&key.name).unwrap(),
            300, // Standard value according to RFC 2845, Sec. 6
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
                Some(RData::A(addr)) => Some(addr.0.into()),
                Some(RData::AAAA(addr)) => Some(addr.0.into()),
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
            IpAddr::V4(addr) => RData::A(A(addr)),
            IpAddr::V6(addr) => RData::AAAA(AAAA(addr)),
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
