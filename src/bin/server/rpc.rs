use std::convert::From;
use std::net::SocketAddr;

use drop::crypto::key::exchange::{self, Exchanger};
use drop::net::{ConnectorExt, TcpConnector, TcpListener};
use drop::system::{AllSampler, Handle, NetworkSender, System, SystemManager};
use futures::future;
use futures::StreamExt;
use murmur::MurmurConfig;
use sieve::SieveConfig;
use contagion::{Contagion,ContagionConfig, ContagionMessage};

use super::accounts::{self, Accounts};
use super::config;
use at2_node::proto;

use snafu::{OptionExt, ResultExt, Snafu};
use tonic::Response;
use tracing::warn;

#[derive(Snafu, Debug)]
pub enum ProtoError {
    #[snafu(display("invalid request"))]
    InvalidRequest,
    #[snafu(display("invalid serialization: {}", source))]
    InvalidSerialization { source: bincode::Error },
}

#[derive(Snafu, Debug)]
pub enum Error {
    #[snafu(display("new service: {}", source))]
    ServiceNew { source: drop::net::ListenerError },
    #[snafu(display("service: process transaction: {}", source))]
    ProcessTransaction { source: accounts::Error },
}

type M = at2_node::Transaction;

#[derive(Clone)]
pub struct Service {
    handle: contagion::ContagionHandle<M, NetworkSender<ContagionMessage<M>>, contagion::Fixed>,
    accounts: Accounts,
}

impl Service {
    pub async fn new(
        listener_addr: SocketAddr,
        network_keypair: exchange::KeyPair,
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

        let listener_errors = system.add_listener(listener).await;
        tokio::spawn(async move {
            listener_errors
                .for_each(|err| {
                    warn!("listener error: {}", err);
                    future::ready(())
                })
                .await
        });

        let manager = SystemManager::new(system);

        let sieve = Contagion::new(
            contagion::Fixed::new_local(),
            ContagionConfig {
                sieve: SieveConfig {
                    sieve_sample_size: network.len(),
                    echo_threshold: network.len(),
                    murmur: MurmurConfig {
                        murmur_gossip_size: network.len(),
                        ..Default::default()
                    },
                },
                contagion_sample_size: network.len(),
                ready_threshold: network.len()
            },
        );

        let sampler = AllSampler::default();
        let mut handle = manager.run(sieve, sampler, num_cpus::get()).await;

        let handle_errors = handle.errors();
        tokio::spawn(async move {
            if let Some(stream) = handle_errors {
                stream
                    .for_each(|err| {
                        warn!("handle error: {}", err);
                        future::ready(())
                    })
                    .await
            }
        });

        let service = Self {
            handle: handle.processor_handle(),
            accounts: Accounts::new(),
        };
        service.spawn();

        Ok(service)
    }

    fn spawn(&self) {
        let mut service = self.clone();

        tokio::spawn(async move {
            loop {
                match service.handle.deliver().await {
                    Err(contagion::ContagionError::Channel) => break,
                    Err(err) => {
                        warn!("deliver batch: {}", err);
                        continue;
                    }
                    Ok(batch) => {
                        for payload in batch.iter() {
                            if let Err(err) = service
                                .process_payload(payload)
                                .await
                                .context(ProcessTransaction)
                            {
                                warn!("{}", err);
                            }
                        }
                    }
                };
            }
        });
    }

    async fn process_payload(
        &self,
        payload: &sieve::Payload<at2_node::Transaction>,
    ) -> Result<(), accounts::Error> {
        self.accounts
            .transfer(
                Box::new(*payload.sender()),
                payload.sequence(),
                Box::new(payload.payload().recipient),
                payload.payload().amount,
            )
            .await
    }
}

impl From<ProtoError> for tonic::Status {
    fn from(err: ProtoError) -> Self {
        Self::invalid_argument(err.to_string())
    }
}
impl From<accounts::Error> for tonic::Status {
    fn from(err: accounts::Error) -> Self {
        Self::invalid_argument(err.to_string())
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

        self.handle
            .clone()
            .broadcast(&sieve::Payload::new(
                bincode::deserialize(&message.sender).context(InvalidSerialization)?,
                message.sequence,
                at2_node::Transaction {
                    recipient: bincode::deserialize(&request_transaction.recipient)
                        .context(InvalidSerialization)?,
                    amount: request_transaction.amount,
                },
                bincode::deserialize(&message.signature).context(InvalidSerialization)?,
            ))
            .await
            .expect("broadcasting failed");

        Ok(Response::new(proto::SendAssetReply {}))
    }

    async fn get_balance(
        &self,
        request: tonic::Request<proto::GetBalanceRequest>,
    ) -> Result<tonic::Response<proto::GetBalanceReply>, tonic::Status> {
        Ok(Response::new(proto::GetBalanceReply {
            amount: self
                .accounts
                .get_balance(
                    bincode::deserialize(&request.get_ref().sender)
                        .context(InvalidSerialization)?,
                )
                .await?,
        }))
    }
}
