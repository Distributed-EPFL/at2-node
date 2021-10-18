use std::io;

use url::Url;

use drop::crypto::sign;

use snafu::{ResultExt, Snafu};

#[derive(serde::Deserialize, serde::Serialize)]
pub struct Config {
    #[serde(with = "serde_str")]
    pub rpc_address: Url,
    #[serde(with = "hex")]
    pub private_key: sign::PrivateKey,
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
