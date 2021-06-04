use std::collections::HashMap;
use std::convert::From;
use std::net::SocketAddr;

use drop::crypto::key::exchange::{self, Exchanger};
use drop::crypto::sign;
use drop::net::{ConnectorExt, TcpConnector, TcpListener};
use drop::system::{AllSampler, Handle, NetworkSender, System, SystemManager};
use futures::future;
use futures::StreamExt;
use murmur::MurmurConfig;
use sieve::{self, Sieve, SieveConfig, SieveMessage};

use super::account::{self, Account};
use super::config;
use at2_node::proto;

use snafu::{OptionExt, ResultExt, Snafu};
use tonic::Response;
use tracing::{info, warn};

#[derive(Snafu, Debug)]
pub enum SendAssetError {
    #[snafu(display("invalid request"))]
    InvalidRequest,
    #[snafu(display("invalid serialization: {}", source))]
    InvalidSerialization { source: bincode::Error },
}

#[derive(Snafu, Debug)]
pub enum ProcessTransactionError {
    #[snafu(display("no such account: {}", pubkey))]
    NoSuchAccount { pubkey: sign::PublicKey },
    #[snafu(display("account modification: {}", source))]
    AccountModification { source: account::Error },
}

#[derive(Snafu, Debug)]
pub enum Error {
    #[snafu(display("new service: {}", source))]
    ServiceNew { source: drop::net::ListenerError },
    #[snafu(display("service: send asset: {}", source))]
    ServiceSendAsset { source: SendAssetError },
    #[snafu(display("service: process transaction: {}", source))]
    ServiceProcessTransaction { source: ProcessTransactionError },
}

type M = at2_node::Transaction;

pub struct Service {
    handle: sieve::SieveHandle<M, NetworkSender<SieveMessage<M>>, sieve::Fixed>,
}

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
            sign_keypair,
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

        let system_handle = manager.run(sieve, sampler, num_cpus::get()).await;

        let mut handle = system_handle.processor_handle();

        tokio::spawn(async move {
            let mut accounts = HashMap::<sign::PublicKey, Account>::default();

            loop {
                match handle.deliver().await {
                    Err(sieve::SieveError::Channel) => break,
                    Err(err) => {
                        warn!("deliver batch: {}", err);
                        continue;
                    }
                    Ok(batch) => batch.iter().for_each(|payload| {
                        if let Err(err) = Self::process_payload(&mut accounts, payload)
                            .context(ServiceProcessTransaction)
                        {
                            warn!("{}", err);
                        }
                    }),
                };
            }
        });

        Ok(Self {
            handle: system_handle.processor_handle(),
        })
    }

    fn process_payload(
        accounts: &mut HashMap<sign::PublicKey, Account>,
        payload: &sieve::Payload<at2_node::Transaction>,
    ) -> Result<(), ProcessTransactionError> {
        let transaction = payload.payload();
        let (sender, recipient) = (*payload.sender(), transaction.recipient);

        // TODO remove me when create_account is done
        let initial_account = Account::new();

        let sender_account = accounts.get(&sender).unwrap_or(&initial_account);
        let recipient_account = accounts.get(&recipient).unwrap_or(&initial_account);

        let new_sender_account = sender_account
            .debit(payload.sequence(), transaction.amount)
            .context(AccountModification)?;
        let new_recipient_account = recipient_account
            .credit(payload.sequence(), transaction.amount)
            .context(AccountModification)?;

        accounts.insert(sender, new_sender_account);
        accounts.insert(recipient, new_recipient_account);

        info!("{} -> {}: {}", sender, recipient, transaction.amount);

        Ok(())
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
