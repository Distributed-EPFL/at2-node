use std::io;

use drop::crypto::sign;
use http::Uri;
use snafu::{ResultExt, Snafu};

#[derive(serde::Deserialize, serde::Serialize)]
pub struct Config {
    #[serde(with = "serde_str")]
    pub rpc_address: Uri,
    #[serde(with = "hex")]
    pub private_key: sign::PrivateKey,
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
