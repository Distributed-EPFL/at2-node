use std::convert::From;
use std::net::SocketAddr;

use drop::crypto::key::exchange::{self, Exchanger};
use drop::crypto::sign;
use drop::net::{ConnectorExt, TcpConnector, TcpListener};
use drop::system::{AllSampler, Handle, NetworkSender, System, SystemManager};
use murmur::MurmurConfig;
use sieve::{self, Sieve, SieveConfig, SieveMessage};

use at2_node::proto;

use snafu::{OptionExt, ResultExt, Snafu};
use tonic::Response;

#[derive(Snafu, Debug)]
pub enum SendAssetError {
    #[snafu(display("invalid request"))]
    InvalidRequest,
    #[snafu(display("invalid serialization: {}", source))]
    InvalidSerialization { source: bincode::Error },
}

#[derive(Snafu, Debug)]
pub enum Error {
    #[snafu(display("new service: {}", source))]
    ServiceNew { source: drop::net::ListenerError },
    #[snafu(display("service: send asset: {}", source))]
    ServiceSendAsset { source: SendAssetError },
}

type M = at2_node::Transaction;

pub struct Service {
    handle: sieve::SieveHandle<M, NetworkSender<SieveMessage<M>>, sieve::Fixed>,
}

use super::config;

impl Service {
    pub async fn new(
        listener_addr: SocketAddr,
        network_keypair: exchange::KeyPair,
        sign_keypair: sign::KeyPair,
        network: Vec<config::Node>,
    ) -> Result<Self, Error> {
        let exchanger = Exchanger::new(network_keypair);

        let listener = TcpListener::new(listener_addr, exchanger.clone())
            .await
            .context(ServiceNew)?;

        let (addrs, keys): (Vec<_>, Vec<_>) = network
            .iter()
            .map(|node| (node.address, node.public_key))
            .unzip();
        let connector = TcpConnector::new(exchanger).retry();
        // TODO readd connections if dropped
        let mut system = System::new_with_connector(&connector, keys, addrs).await;

        let _ = system.add_listener(listener).await;
        let manager = SystemManager::new(system);

        let sieve = Sieve::new(
            sign_keypair,
            sieve::Fixed::new_local(),
            SieveConfig {
                sieve_sample_size: 1,
                murmur: MurmurConfig {
                    murmur_gossip_size: network.len(),
                    ..Default::default()
                },
                ..Default::default()
            },
        );

        let sampler = AllSampler::default();

        // TODO log errors from manager
        Ok(Self {
            handle: manager
                .run(sieve, sampler, num_cpus::get())
                .await
                .processor_handle(),
        })
    }
}

impl From<SendAssetError> for tonic::Status {
    fn from(error: SendAssetError) -> Self {
        Self::invalid_argument(error.to_string())
    }
}

#[tonic::async_trait]
impl proto::At2 for Service {
    async fn send_asset(
        &self,
        request: tonic::Request<proto::SendAssetRequest>,
    ) -> Result<tonic::Response<proto::SendAssetReply>, tonic::Status> {
        let message = request.into_inner();
        let request_transaction = message.transaction.context(InvalidRequest)?;

        let transaction = at2_node::Transaction {
            recipient: bincode::deserialize(&request_transaction.recipient)
                .context(InvalidSerialization)?,
            amount: request_transaction.amount,
        };

        let sender = bincode::deserialize(&message.sender).context(InvalidSerialization)?;
        let signature = bincode::deserialize(&message.signature).context(InvalidSerialization)?;
        let payload = sieve::Payload::new(sender, message.sequence, transaction, signature);

        self.handle
            .clone()
            .broadcast(&payload)
            .await
            .expect("broadcasting failed");

        Ok(Response::new(proto::SendAssetReply {}))
    }
}
