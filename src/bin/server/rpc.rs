use std::convert::From;
use std::net::SocketAddr;

use drop::crypto::key::exchange::{self, Exchanger};
use drop::net::{ConnectorExt, TcpConnector, TcpListener};
use drop::system::{AllSampler, Handle, NetworkSender, System, SystemManager};
use futures::future;
use futures::StreamExt;
use murmur::MurmurConfig;
use sieve::{self, Sieve, SieveConfig, SieveMessage};
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use super::accounts::{self, Accounts};
use super::config;
use at2_node::proto;

use snafu::{OptionExt, ResultExt, Snafu};
use tonic::Response;
use tracing::warn;

#[derive(Snafu, Debug)]
pub enum AccountsAgentError {
    #[snafu(display("account modification: {}", source))]
    AccountModification { source: accounts::Error },

    #[snafu(display("gone on send: {}", source))]
    GoneOnSend {
        source: mpsc::error::SendError<accounts::Commands>,
    },
    #[snafu(display("gone on recv: {}", source))]
    GoneOnRecv { source: oneshot::error::RecvError },
}

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
    ProcessTransaction { source: AccountsAgentError },

    #[snafu(display("service: send asset: {}", source))]
    SendAsset { source: ProtoError },
}

type M = at2_node::Transaction;

#[derive(Clone)]
pub struct Service {
    handle: sieve::SieveHandle<M, NetworkSender<SieveMessage<M>>, sieve::Fixed>,
    accounts_agent: mpsc::Sender<accounts::Commands>,
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

        let sieve = Sieve::new(
            sieve::Fixed::new_local(),
            SieveConfig {
                sieve_sample_size: network.len(),
                echo_threshold: network.len(),
                murmur: MurmurConfig {
                    murmur_gossip_size: network.len(),
                    ..Default::default()
                },
            },
        );

        let sampler = AllSampler::default();

        let service = Self {
            handle: manager
                .run(sieve, sampler, num_cpus::get())
                .await
                .processor_handle(),
            accounts_agent: Accounts::new().spawn(),
        };
        service.spawn();

        Ok(service)
    }

    fn spawn(&self) {
        let mut service = self.clone();

        tokio::spawn(async move {
            loop {
                match service.handle.deliver().await {
                    Err(sieve::SieveError::Channel) => break,
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
    ) -> Result<(), AccountsAgentError> {
        let (tx, rx) = oneshot::channel();

        self.accounts_agent
            .send(accounts::Commands::Transfer {
                sender: *payload.sender(),
                sender_sequence: payload.sequence(),
                receiver: payload.payload().recipient,
                amount: payload.payload().amount,
                resp: tx,
            })
            .await
            .context(GoneOnSend)?;

        rx.await.context(GoneOnRecv)?.context(AccountModification)
    }
}

impl From<ProtoError> for tonic::Status {
    fn from(err: ProtoError) -> Self {
        Self::invalid_argument(err.to_string())
    }
}

impl From<AccountsAgentError> for tonic::Status {
    fn from(err: AccountsAgentError) -> Self {
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

    async fn get_balance(
        &self,
        request: tonic::Request<proto::GetBalanceRequest>,
    ) -> Result<tonic::Response<proto::GetBalanceReply>, tonic::Status> {
        let (tx, rx) = oneshot::channel();

        self.accounts_agent
            .send(accounts::Commands::GetBalance {
                user: bincode::deserialize(&request.get_ref().sender)
                    .context(InvalidSerialization)?,
                resp: tx,
            })
            .await
            .context(GoneOnSend)?;

        Ok(Response::new(proto::GetBalanceReply {
            amount: rx.await.context(GoneOnRecv)?.context(AccountModification)?,
        }))
    }
}
