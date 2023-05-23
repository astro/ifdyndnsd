/// Copied from Andreas Rottmann's tdns-cli (GPL-3)
///
/// An implementation of RFC 2845 for `trust-dns`.
use std::{
    convert::{TryFrom, TryInto},
    fmt,
};

use hmac::digest::InvalidLength;
use hmac::{Hmac, Mac};
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
    InvalidLength(InvalidLength),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Proto(e) => write!(f, "{e}"),
            Error::InvalidLength(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<ProtoError> for Error {
    fn from(e: ProtoError) -> Self {
        Error::Proto(e)
    }
}

impl From<InvalidLength> for Error {
    fn from(e: InvalidLength) -> Self {
        Error::InvalidLength(e)
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
        match self {
            Algorithm::HmacSha224 => &names.sha224,
            Algorithm::HmacSha256 => &names.sha256,
            Algorithm::HmacSha384 => &names.sha384,
            Algorithm::HmacSha512 => &names.sha512,
        }
    }
    pub fn from_name(name: &rr::Name) -> Result<Algorithm, UnknownAlgorithm> {
        let names = Lazy::force(&ALGO_NAMES);
        for (algo_name, algo) in &[
            (&names.sha224, Algorithm::HmacSha224),
            (&names.sha256, Algorithm::HmacSha256),
            (&names.sha384, Algorithm::HmacSha384),
            (&names.sha512, Algorithm::HmacSha512),
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

pub fn create_signature(
    msg: &op::Message,
    time_signed: u64,
    key: &Key,
) -> Result<rr::Record, Error> {
    let tsig = match key.algorithm {
        Algorithm::HmacSha224 => create_tsig::<Hmac<sha2::Sha224>>(msg, time_signed, key)?,
        Algorithm::HmacSha256 => create_tsig::<Hmac<sha2::Sha256>>(msg, time_signed, key)?,
        Algorithm::HmacSha384 => create_tsig::<Hmac<sha2::Sha384>>(msg, time_signed, key)?,
        Algorithm::HmacSha512 => create_tsig::<Hmac<sha2::Sha512>>(msg, time_signed, key)?,
    };
    let mut record = rr::Record::from_rdata(key.name.clone(), 0, tsig.try_into()?);
    record.set_dns_class(rr::DNSClass::ANY);
    Ok(record)
}

#[derive(Debug)]
struct Tsig {
    algorithm_name: rr::Name,
    time_signed: u64, // This is a actually a 48-bit value
    fudge: u16,
    mac: Vec<u8>,
    original_id: u16,
    error: op::ResponseCode,
}

impl Tsig {
    fn new(
        algorithm_name: rr::Name,
        time_signed: u64, // This is a actually a 48-bit value
        fudge: u16,
        mac: Vec<u8>,
        original_id: u16,
        error: op::ResponseCode,
    ) -> Self {
        Tsig {
            algorithm_name,
            time_signed,
            fudge,
            mac,
            original_id,
            error,
        }
    }
}

impl TryFrom<Tsig> for rr::RData {
    type Error = Error;

    fn try_from(tsig: Tsig) -> Result<Self, Self::Error> {
        let mut encoded = Vec::new();
        let mut bin_encoder = BinEncoder::new(&mut encoded);
        bin_encoder.set_canonical_names(true);
        tsig.emit(&mut bin_encoder)?;
        Ok(rr::RData::Unknown {
            code: 250,
            rdata: rr::rdata::null::NULL::with(encoded),
        })
    }
}

impl BinEncodable for Tsig {
    fn emit(&self, bin_encoder: &mut BinEncoder) -> ProtoResult<()> {
        self.algorithm_name.emit(bin_encoder)?;
        emit_u48(bin_encoder, self.time_signed)?;
        bin_encoder.emit_u16(self.fudge)?;
        bin_encoder.emit_u16(self.mac.len() as u16)?;
        bin_encoder.emit_vec(&self.mac)?;
        bin_encoder.emit_u16(self.original_id)?;
        bin_encoder.emit_u16(self.error.into())?;
        bin_encoder.emit_u16(0)?; // Other data is of length 0
        Ok(())
    }
}

fn emit_u48(bin_encoder: &mut BinEncoder, n: u64) -> ProtoResult<()> {
    bin_encoder.emit_u16((n >> 32) as u16)?;
    bin_encoder.emit_u32(n as u32)?;
    Ok(())
}

fn create_tsig<T: Mac + hmac::digest::KeyInit>(
    msg: &op::Message,
    time_signed: u64,
    key: &Key,
) -> Result<Tsig, Error> {
    let mut encoded = Vec::new(); // TODO: initial capacity?
    let mut bin_encoder = BinEncoder::new(&mut encoded);
    let fudge = 300; // FIXME: fudge hardcoded
                     // See RFC 2845, section 3.4. The "whole and complete message" in wire
                     // format, before adding the Tsig RR.
    msg.emit(&mut bin_encoder)?;
    //  3.4.2. Tsig Variables
    //
    // Source       Field Name       Notes
    // -----------------------------------------------------------------------
    // Tsig RR      NAME             Key name, in canonical wire format
    // Tsig RR      CLASS            (Always ANY in the current specification)
    // Tsig RR      TTL              (Always 0 in the current specification)
    // Tsig RDATA   Algorithm Name   in canonical wire format
    // Tsig RDATA   Time Signed      in network byte order
    // Tsig RDATA   Fudge            in network byte order
    // Tsig RDATA   Error            in network byte order
    // Tsig RDATA   Other Len        in network byte order
    // Tsig RDATA   Other Data       exactly as transmitted
    bin_encoder.set_canonical_names(true);
    key.name.emit(&mut bin_encoder)?;
    rr::DNSClass::ANY.emit(&mut bin_encoder)?;
    bin_encoder.emit_u32(0)?; // TTL
    key.algorithm.as_name().emit(&mut bin_encoder)?;
    emit_u48(&mut bin_encoder, time_signed)?;
    bin_encoder.emit_u16(fudge)?;
    let rcode = op::ResponseCode::NoError;
    bin_encoder.emit_u16(rcode.into())?;
    bin_encoder.emit_u16(0)?; // Other data is of length 0
    let hmac = {
        let mut mac = <T as Mac>::new_from_slice(&key.secret)?;
        mac.update(&encoded);
        mac.finalize().into_bytes().to_vec()
    };
    Ok(Tsig::new(
        key.algorithm.as_name().clone(),
        time_signed,
        fudge,
        hmac,
        msg.id(),
        rcode,
    ))
}
