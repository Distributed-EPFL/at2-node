use std::{io, net::SocketAddr};

use drop::crypto::{key::exchange, sign};
use snafu::{ResultExt, Snafu};

#[derive(serde::Deserialize, serde::Serialize)]
pub struct ConfigAddresses {
    pub node: SocketAddr,
    pub rpc: SocketAddr,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct ConfigKeys {
    #[serde(with = "hex")]
    pub sign: sign::PrivateKey,
    pub network: exchange::PrivateKey,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct Config {
    pub addresses: ConfigAddresses,
    pub keys: ConfigKeys,
    // FIXME toml fails with empty Vec alexcrichton/toml-rs#384
    #[serde(skip_serializing_if = "Vec::is_empty", default = "Vec::default")]
    pub nodes: Vec<Node>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct Node {
    pub address: SocketAddr,
    #[serde(with = "hex")]
    pub public_key: exchange::PublicKey,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct Nodes {
    pub nodes: Vec<Node>,
}

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("encode: {}", source))]
    Encode { source: toml::ser::Error },
    #[snafu(display("write: {}", source))]
    Write { source: io::Error },

    #[snafu(display("read: {}", source))]
    Read { source: io::Error },
    #[snafu(display("decode: {}", source))]
    Decode { source: toml::de::Error },
}

pub fn from_reader(mut reader: impl io::Read) -> Result<Config, Error> {
    let mut buffer = String::new();
    reader.read_to_string(&mut buffer).context(Read)?;

    let config: Config = toml::from_str(&buffer).context(Decode)?;

    Ok(config)
}

impl Config {
    pub fn to_writer(&self, mut writer: impl io::Write) -> Result<(), Error> {
        let encoded = toml::to_vec(&self).context(Encode)?;

        writer.write_all(&encoded).context(Write)?;

        Ok(())
    }
}

impl Nodes {
    // TODO wrapped into a vec, get rid of it when toml with empty vec works
    pub fn to_writer(&self, mut writer: impl io::Write) -> Result<(), Error> {
        let encoded = toml::to_vec(&self).context(Encode)?;

        writer.write_all(&encoded).context(Write)?;

        Ok(())
    }
}
