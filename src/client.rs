use drop::crypto::sign;
use http::Uri;
use snafu::{ResultExt, Snafu};

use crate::{
    proto::{at2_client::At2Client, *},
    Transaction,
};

#[derive(Debug, Snafu)]
pub enum Error {
    #[cfg(not(target_family = "wasm"))]
    Transport {
        source: tonic::transport::Error,
    },
    Deserialize {
        source: bincode::Error,
    },
    Serialize {
        source: bincode::Error,
    },
    Rpc {
        source: tonic::Status,
    },
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Clone)]
pub struct Client(
    #[cfg(target_family = "wasm")] At2Client<grpc_web_client::Client>,
    #[cfg(not(target_family = "wasm"))] At2Client<tonic::transport::Channel>,
);

impl Client {
    pub fn new(uri: Uri) -> Result<Self> {
        let mut url_string = uri.to_string();
        if uri.path() == "/" {
            // TODO fix upstream handling
            url_string.pop();
        }

        #[cfg(target_family = "wasm")]
        let connection = grpc_web_client::Client::new(url_string);
        #[cfg(not(target_family = "wasm"))]
        let connection = tonic::transport::Channel::builder(uri)
            .connect_lazy()
            .context(Transport)?;

        Ok(Self(At2Client::new(connection)))
    }

    pub async fn send_asset(
        &mut self,
        user: &sign::KeyPair,
        sequence: sieve::Sequence,
        recipient: sign::PublicKey,
        amount: u64,
    ) -> Result<()> {
        let message = Transaction { recipient, amount };
        let signature = user.sign(&message).expect("sign failed");

        self.0
            .send_asset(tonic::Request::new(SendAssetRequest {
                sender: bincode::serialize(&user.public()).context(Serialize)?,
                sequence,
                receiver: bincode::serialize(&recipient).context(Serialize)?,
                amount,
                signature: bincode::serialize(&signature).context(Serialize)?,
            }))
            .await
            .context(Rpc)
            .map(|_| ())
    }

    pub async fn get_balance(&mut self, user: &sign::PublicKey) -> Result<u64> {
        self.0
            .get_balance(tonic::Request::new(GetBalanceRequest {
                sender: bincode::serialize(user).context(Serialize)?,
            }))
            .await
            .context(Rpc)
            .map(|reply| reply.get_ref().amount)
    }

    pub async fn get_last_sequence(&mut self, user: &sign::PublicKey) -> Result<sieve::Sequence> {
        self.0
            .get_last_sequence(tonic::Request::new(GetLastSequenceRequest {
                sender: bincode::serialize(user).context(Serialize)?,
            }))
            .await
            .context(Rpc)
            .map(|reply| reply.get_ref().sequence)
    }
}
