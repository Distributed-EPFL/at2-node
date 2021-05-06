use std::io;
use std::net::SocketAddr;

use snafu::{ResultExt, Snafu};

use drop::crypto::{key::exchange, sign};

#[derive(serde::Deserialize, serde::Serialize)]
pub struct ConfigAddresses {
    pub node: SocketAddr,
    pub rpc: SocketAddr,
}

// TODO remove when exchange::SecretKey can be used to generate exchange::PublicKey
#[derive(serde::Deserialize, serde::Serialize)]
pub struct ConfigKeysNetwork {
    #[serde(with = "serde_str")]
    pub public: exchange::PublicKey,
    #[serde(with = "serde_str")]
    pub secret: exchange::SecretKey,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct ConfigKeys {
    #[serde(with = "serde_str")]
    pub sign: sign::SecretKey,
    pub network: ConfigKeysNetwork,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct Config {
    pub addresses: ConfigAddresses,
    pub keys: ConfigKeys,
    // FIXME toml fails with empty Vec alexcrichton/toml-rs#384
    #[serde(skip_serializing_if = "Vec::is_empty", default = "Vec::default")]
    pub nodes: Vec<Node>,
}

impl From<exchange::KeyPair> for ConfigKeysNetwork {
    fn from(keypair: exchange::KeyPair) -> Self {
        Self {
            public: *keypair.public(),
            secret: keypair.secret().clone(),
        }
    }
}

impl From<ConfigKeysNetwork> for exchange::KeyPair {
    fn from(config: ConfigKeysNetwork) -> Self {
        exchange::KeyPair::new(config.secret, config.public)
    }
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct Node {
    pub address: SocketAddr,
    #[serde(with = "serde_str")]
    pub public_key: exchange::PublicKey,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct Nodes {
    pub nodes: Vec<Node>,
}

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("encode: {}", source))]
    EncodeConfig { source: toml::ser::Error },
    #[snafu(display("write: {}", source))]
    WriteConfig { source: io::Error },

    #[snafu(display("read: {}", source))]
    ReadConfig { source: io::Error },
    #[snafu(display("decode: {}", source))]
    DecodeConfig { source: toml::de::Error },
}

pub fn from_reader(mut reader: impl io::Read) -> Result<Config, Error> {
    let mut buffer = String::new();
    reader.read_to_string(&mut buffer).context(ReadConfig)?;

    let config: Config = toml::from_str(&buffer).context(DecodeConfig)?;

    Ok(config)
}

impl Config {
    pub fn to_writer(&self, mut writer: impl io::Write) -> Result<(), Error> {
        let encoded = toml::to_vec(&self).context(EncodeConfig)?;

        writer.write_all(&encoded).context(WriteConfig)?;

        Ok(())
    }
}

impl Nodes {
    // TODO wrapped into a vec, get rid of it when toml with empty vec works
    pub fn to_writer(&self, mut writer: impl io::Write) -> Result<(), Error> {
        let encoded = toml::to_vec(&self).context(EncodeConfig)?;

        writer.write_all(&encoded).context(WriteConfig)?;

        Ok(())
    }
}
