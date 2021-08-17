/// Copied from Andreas Rottmann's tdns-cli (GPL-3)
///
/// An implementation of RFC 2845 for `trust-dns`.
use std::{
    convert::{TryFrom, TryInto},
    fmt,
};

use hmac::crypto_mac::InvalidKeyLength;
use hmac::{Hmac, Mac, NewMac};
use once_cell::sync::Lazy;
use trust_dns_client::{
    op,
    proto::error::{ProtoError, ProtoResult},
    rr,
    serialize::binary::{BinEncodable, BinEncoder},
};

#[derive(Debug)]
pub enum Error {
    Proto(ProtoError),
    InvalidKeyLength(InvalidKeyLength),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Proto(e) => write!(f, "{}", e),
            Error::InvalidKeyLength(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for Error {}

impl From<ProtoError> for Error {
    fn from(e: ProtoError) -> Self {
        Error::Proto(e)
    }
}

impl From<InvalidKeyLength> for Error {
    fn from(e: InvalidKeyLength) -> Self {
        Error::InvalidKeyLength(e)
    }
}

#[derive(Debug, Copy, Clone)]
pub enum Algorithm {
    HmacSha224,
    HmacSha256,
    HmacSha384,
    HmacSha512,
}

struct AlgoNames {
    sha224: rr::Name,
    sha256: rr::Name,
    sha384: rr::Name,
    sha512: rr::Name,
}

static ALGO_NAMES: Lazy<AlgoNames> = Lazy::new(|| AlgoNames {
    // All SHA2-based algorithms are defined in RFC 4635
    sha224: rr::Name::from_ascii("hmac-sha224").unwrap(),
    sha256: rr::Name::from_ascii("hmac-sha256").unwrap(),
    sha384: rr::Name::from_ascii("hmac-sha384").unwrap(),
    sha512: rr::Name::from_ascii("hmac-sha512").unwrap(),
});

impl Algorithm {
    pub fn as_name(self) -> &'static rr::Name {
        let names = Lazy::force(&ALGO_NAMES);
        use Algorithm::*;
        match self {
            HmacSha224 => &names.sha224,
            HmacSha256 => &names.sha256,
            HmacSha384 => &names.sha384,
            HmacSha512 => &names.sha512,
        }
    }
    pub fn from_name(name: &rr::Name) -> Result<Algorithm, UnknownAlgorithm> {
        let names = Lazy::force(&ALGO_NAMES);
        use Algorithm::*;
        for (algo_name, algo) in &[
            (&names.sha224, HmacSha224),
            (&names.sha256, HmacSha256),
            (&names.sha384, HmacSha384),
            (&names.sha512, HmacSha512),
        ] {
            if name == *algo_name {
                return Ok(*algo);
            }
        }
        Err(UnknownAlgorithm)
    }
}

#[derive(Debug)]
pub struct UnknownAlgorithm;

impl fmt::Display for UnknownAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "unknown algorithm")
    }
}

impl std::error::Error for UnknownAlgorithm {}

#[derive(Debug, Clone)]
pub struct Key {
    name: rr::Name,
    algorithm: Algorithm,
    secret: Vec<u8>,
}

impl Key {
    pub fn new<T>(name: rr::Name, algorithm: Algorithm, secret: T) -> Self
    where
        T: Into<Vec<u8>>,
    {
        Key {
            name,
            algorithm,
            secret: secret.into(),
        }
    }
}

pub fn create_signature(msg: &op::Message, time_signed: u64, key: &Key) -> Result<rr::Record, Error> {
    use Algorithm::*;
    let tsig = match key.algorithm {
        HmacSha224 => create_tsig::<Hmac<sha2::Sha224>>(&msg, time_signed, &key)?,
        HmacSha256 => create_tsig::<Hmac<sha2::Sha256>>(&msg, time_signed, &key)?,
        HmacSha384 => create_tsig::<Hmac<sha2::Sha384>>(&msg, time_signed, &key)?,
        HmacSha512 => create_tsig::<Hmac<sha2::Sha512>>(&msg, time_signed, &key)?,
    };
    let mut record = rr::Record::from_rdata(key.name.clone(), 0, tsig.try_into()?);
    record.set_dns_class(rr::DNSClass::ANY);
    Ok(record)
}

#[derive(Debug)]
struct TSIG {
    algorithm_name: rr::Name,
    time_signed: u64, // This is a actually a 48-bit value
    fudge: u16,
    mac: Vec<u8>,
    original_id: u16,
    error: op::ResponseCode,
    other_data: Vec<u8>,
}

impl TSIG {
    fn new(
        algorithm_name: rr::Name,
        time_signed: u64, // This is a actually a 48-bit value
        fudge: u16,
        mac: Vec<u8>,
        original_id: u16,
        error: op::ResponseCode,
        other_data: Vec<u8>,
    ) -> Self {
        TSIG {
            algorithm_name,
            time_signed,
            fudge,
            mac,
            original_id,
            error,
            other_data,
        }
    }
}

impl TryFrom<TSIG> for rr::RData {
    type Error = Error;

    fn try_from(tsig: TSIG) -> Result<Self, Self::Error> {
        let mut encoded = Vec::new();
        let mut encoder = BinEncoder::new(&mut encoded);
        encoder.set_canonical_names(true);
        tsig.emit(&mut encoder)?;
        Ok(rr::RData::Unknown {
            code: 250,
            rdata: rr::rdata::null::NULL::with(encoded),
        })
    }
}

impl BinEncodable for TSIG {
    fn emit(&self, encoder: &mut BinEncoder) -> ProtoResult<()> {
        self.algorithm_name.emit(encoder)?;
        emit_u48(encoder, self.time_signed)?;
        encoder.emit_u16(self.fudge)?;
        encoder.emit_u16(self.mac.len() as u16)?;
        encoder.emit_vec(&self.mac)?;
        encoder.emit_u16(self.original_id)?;
        encoder.emit_u16(self.error.into())?;
        encoder.emit_u16(0)?; // Other data is of length 0
        Ok(())
    }
}

fn emit_u48(encoder: &mut BinEncoder, n: u64) -> ProtoResult<()> {
    encoder.emit_u16((n >> 32) as u16)?;
    encoder.emit_u32(n as u32)?;
    Ok(())
}

fn create_tsig<T: Mac + NewMac>(
    msg: &op::Message,
    time_signed: u64,
    key: &Key,
) -> Result<TSIG, Error> {
    let mut encoded = Vec::new(); // TODO: initial capacity?
    let mut encoder = BinEncoder::new(&mut encoded);
    let fudge = 300; // FIXME: fudge hardcoded
                     // See RFC 2845, section 3.4. The "whole and complete message" in wire
                     // format, before adding the TSIG RR.
    msg.emit(&mut encoder)?;
    //  3.4.2. TSIG Variables
    //
    // Source       Field Name       Notes
    // -----------------------------------------------------------------------
    // TSIG RR      NAME             Key name, in canonical wire format
    // TSIG RR      CLASS            (Always ANY in the current specification)
    // TSIG RR      TTL              (Always 0 in the current specification)
    // TSIG RDATA   Algorithm Name   in canonical wire format
    // TSIG RDATA   Time Signed      in network byte order
    // TSIG RDATA   Fudge            in network byte order
    // TSIG RDATA   Error            in network byte order
    // TSIG RDATA   Other Len        in network byte order
    // TSIG RDATA   Other Data       exactly as transmitted
    encoder.set_canonical_names(true);
    key.name.emit(&mut encoder)?;
    rr::DNSClass::ANY.emit(&mut encoder)?;
    encoder.emit_u32(0)?; // TTL
    key.algorithm.as_name().emit(&mut encoder)?;
    emit_u48(&mut encoder, time_signed)?;
    encoder.emit_u16(fudge)?;
    let rcode = op::ResponseCode::NoError;
    encoder.emit_u16(rcode.into())?;
    encoder.emit_u16(0)?; // Other data is of length 0
    let hmac = {
        let mut mac = T::new_from_slice(&key.secret)?;
        mac.update(&encoded);
        mac.finalize().into_bytes().to_vec()
    };
    Ok(TSIG::new(
        key.algorithm.as_name().clone(),
        time_signed,
        fudge,
        hmac,
        msg.id(),
        rcode,
        Vec::new(),
    ))
}
