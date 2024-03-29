//! Client for connecting to an AT2 node

use drop::crypto::sign;
use http::Uri;
use snafu::{OptionExt, ResultExt, Snafu};

use crate::{
    proto::{at2_client::At2Client, *},
    FullTransaction, ThinTransaction, TransactionState,
};

/// Error generated by this client
#[derive(Debug, Snafu)]
pub enum Error {
    /// Deserialize the server's reply
    Deserialize {
        /// Source of the error
        source: bincode::Error,
    },
    /// Deserializing the timestamp
    DeserializeTimestamp {
        /// Source of the error
        source: chrono::ParseError,
    },
    /// Deserializing the transaction state
    DeserializeState,
    /// Serializing the server's query
    Serialize {
        /// Source of the error
        source: bincode::Error,
    },
    /// Communicating with the server
    #[snafu(display("commnucating with the server: {}", source))]
    Rpc {
        /// Source of the error
        source: tonic::Status,
    },
}

type Result<T> = std::result::Result<T, Error>;

/// gRPC web client for the node
#[derive(Clone)]
pub struct Client(
    #[cfg(target_family = "wasm")] At2Client<grpc_web_client::Client>,
    #[cfg(not(target_family = "wasm"))] At2Client<tonic::transport::Channel>,
);

impl Client {
    /// Create a new client connecting to the given [`Uri`]
    pub fn new(uri: Uri) -> Self {
        let mut url_string = uri.to_string();
        if uri.path() == "/" {
            // TODO fix upstream handling
            url_string.pop();
        }

        #[cfg(target_family = "wasm")]
        let connection = grpc_web_client::Client::new(url_string);
        #[cfg(not(target_family = "wasm"))]
        let connection = tonic::transport::Channel::builder(uri).connect_lazy();

        Self(At2Client::new(connection))
    }

    /// Send a given number of asset to the given user.
    ///
    /// `sequence` is counter used by the sender.
    /// You should increase it by one for each new transaction you want to send.
    pub async fn send_asset(
        &mut self,
        user: &sign::KeyPair,
        sequence: sieve::Sequence,
        recipient: sign::PublicKey,
        amount: u64,
    ) -> Result<()> {
        let message = ThinTransaction { recipient, amount };
        let signature = user.sign(&message).expect("sign failed");

        self.0
            .send_asset(tonic::Request::new(SendAssetRequest {
                sender: bincode::serialize(&user.public()).context(Serialize)?,
                sequence,
                recipient: bincode::serialize(&recipient).context(Serialize)?,
                amount,
                signature: bincode::serialize(&signature).context(Serialize)?,
            }))
            .await
            .context(Rpc)
            .map(|_| ())
    }

    /// Return the balance of the user
    pub async fn get_balance(&mut self, user: &sign::PublicKey) -> Result<u64> {
        self.0
            .get_balance(tonic::Request::new(GetBalanceRequest {
                sender: bincode::serialize(user).context(Serialize)?,
            }))
            .await
            .context(Rpc)
            .map(|reply| reply.get_ref().amount)
    }

    /// Get the latest used sequence
    pub async fn get_last_sequence(&mut self, user: &sign::PublicKey) -> Result<sieve::Sequence> {
        self.0
            .get_last_sequence(tonic::Request::new(GetLastSequenceRequest {
                sender: bincode::serialize(user).context(Serialize)?,
            }))
            .await
            .context(Rpc)
            .map(|reply| reply.get_ref().sequence)
    }

    /// Get the number of recently processed transactions
    pub async fn get_latest_transactions(&mut self) -> Result<Vec<FullTransaction>> {
        use full_transaction::State;

        self.0
            .get_latest_transactions(tonic::Request::new(GetLatestTransactionsRequest {}))
            .await
            .context(Rpc)?
            .into_inner()
            .transactions
            .iter()
            .map(|tx| {
                Ok(FullTransaction {
                    timestamp: chrono::DateTime::parse_from_rfc3339(&tx.timestamp)
                        .context(DeserializeTimestamp)?
                        .into(),
                    sender: bincode::deserialize(&tx.sender).context(Deserialize)?,
                    sender_sequence: tx.sender_sequence,
                    recipient: bincode::deserialize(&tx.recipient).context(Deserialize)?,
                    amount: tx.amount,
                    state: match State::from_i32(tx.state).context(DeserializeState)? {
                        State::Pending => TransactionState::Pending,
                        State::Success => TransactionState::Success,
                        State::Failure => TransactionState::Failure,
                    },
                })
            })
            .collect()
    }
}
