use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::net::{IpAddr, Ipv6Addr};
use serde_derive::Deserialize;

#[derive(Debug, Deserialize)]
pub struct TsigKey {
    pub server: IpAddr,
    pub name: String,
    pub alg: String,
    pub secret: Option<String>,
    #[serde(rename = "secret-base64")]
    pub secret_base64: Option<String>,
}

impl TsigKey {
    pub fn get_secret(&self) -> Vec<u8> {
        match (&self.secret, &self.secret_base64) {
            (Some(_), Some(_)) =>
                panic!("Both secret and secret_base64 configured for key {}", self.name),
            (None, None) =>
                panic!("No secret or secret_base64 configured for key {}", self.name),
            (Some(secret), _) =>
                secret.bytes().collect::<Vec<u8>>(),
            (_, Some(secret_base64)) =>
                base64::decode(secret_base64).unwrap()
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Interface {
    pub key: String,
    pub name: String,
    pub interface: String,
    pub scope: Option<String>,
    pub neighbors: Option<HashMap<String, Ipv6Addr>>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub keys: HashMap<String, TsigKey>,
    pub a: Option<Vec<Interface>>,
    pub aaaa: Option<Vec<Interface>>,
}

pub fn load(filename: &str) -> Result<Config, String> {
    let mut f = File::open(filename)
        .map_err(|e| format!("{}", e))?;
    let mut buf = String::new();
    f.read_to_string(&mut buf)
        .map_err(|e| format!("{}", e))?;
    let config = toml::from_str(&buf)
        .map_err(|e| format!("{}", e))?;
    Ok(config)
}
