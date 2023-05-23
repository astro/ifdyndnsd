use base64::engine::general_purpose;
use base64::Engine;
use serde_derive::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::net::{IpAddr, Ipv6Addr};

#[derive(Debug, Deserialize)]
pub struct TsigKey {
    pub server: IpAddr,
    pub name: String,
    pub alg: String,
    pub secret: Option<String>,
    #[serde(rename = "secret-base64")]
    pub secret_base64: Option<String>,
    #[serde(rename = "secret-file")]
    pub secret_file: Option<String>,
    #[serde(rename = "secret-file-base64")]
    pub secret_file_base64: Option<String>,
}

impl TsigKey {
    #[must_use]
    /// # Panics
    ///
    /// - `secret_base64` could not be decoded from base64.
    /// - File where `secret_file` or `secret_file_base64` points to does not exist or the user does not have permission to read it.
    /// - Contents of file where `secret_file_base64` could not be decoded from base64.
    ///
    pub fn get_secret(&self) -> Vec<u8> {
        match (&self.secret, &self.secret_base64, &self.secret_file, &self.secret_file_base64) {
            (Some(secret), None, None,None) => secret.bytes().collect::<Vec<u8>>(),
            (None, Some(secret_base64), None, None) => general_purpose::STANDARD.decode(secret_base64).unwrap(),
            (None, None, Some(secret_file), None) => {
                let file = File::open(secret_file).map_err(|e| format!("Failed to open the specified secret-file: {e}")).unwrap();
                file.bytes().map(std::result::Result::unwrap).collect()
            },
            (None, None, None, Some(secret_file_base64)) => {
                let mut file = File::open(secret_file_base64).map_err(|e| format!("Failed to open the specified secret-file-base64: {e}")).unwrap();
                let mut buf = Vec::new();
                file.read_to_end(&mut buf).unwrap();
                general_purpose::STANDARD.decode(buf).unwrap()
            },
            (None, None, None, None) => panic!(
                "Neither secret nor secret-base64 nor secret-file nor secret-file-base64 configured for key {}. Configure one of the secret parameters.",
                self.name
            ),
            (_, _, _, _) => panic!(
                "More than one of the parameters secret, secret-base64 or secret-file configured for key {}.
                Configure exactly one of the secret parameters.",
                self.name
            ),
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
/// # Errors
///
/// Will return `Err` if
///
/// - `config_file` does not exist or the user does not have permission to read it.
/// - Data inside `config_file` is not valid UTF-8.
/// - Data inside `config_file` could not be deserialized as TOML.
///
pub fn load(filename: &str) -> Result<Config, String> {
    let mut f = File::open(filename).map_err(|e| format!("{e}"))?;
    let mut buf = String::new();
    f.read_to_string(&mut buf).map_err(|e| format!("{e}"))?;
    let config = toml::from_str(&buf).map_err(|e| format!("{e}"))?;
    Ok(config)
}
