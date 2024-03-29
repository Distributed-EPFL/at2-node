use std::{
    cmp::Reverse,
    collections::BinaryHeap,
    fmt,
    time::{Duration, Instant},
};

use at2_node::{
    proto::{self, *},
    ThinTransaction, TransactionState,
};
use contagion::{Contagion, ContagionConfig, ContagionMessage};
use drop::{
    crypto::{
        key::exchange::{self, Exchanger},
        sign,
    },
    net::{ConnectorExt, ResolveConnector, TcpConnector, TcpListener},
    system::{AllSampler, Handle, NetworkSender, System, SystemManager},
};
use futures::{future, StreamExt};
use murmur::MurmurConfig;
use sieve::SieveConfig;
use snafu::{ResultExt, Snafu};
use tokio::net;
use tonic::Response;
use tracing::{info, warn};

use super::{
    accounts::{self, Accounts},
    config,
    recent_transactions::{self, RecentTransactions},
};

const TRANSACTION_TTL: Duration = Duration::from_secs(60);

#[derive(Snafu, Debug)]
pub enum ProtoError {
    #[snafu(display("deserialize: {}", source))]
    Deserialize { source: bincode::Error },
    #[snafu(display("serialize: {}", source))]
    Serialize { source: bincode::Error },
}

#[derive(Snafu, Debug)]
pub enum ProcessTransactionError {
    #[snafu(display("handle by acounts: {}", source))]
    ProcessTxForAccounts { source: accounts::Error },
    #[snafu(display("handle by recent transactions: {}", source))]
    ProcessTxForRecent { source: recent_transactions::Error },
}

#[derive(Snafu, Debug)]
pub enum Error {
    #[snafu(display("new service: {}", source))]
    ServiceNew { source: drop::net::ListenerError },
    #[snafu(display("service: process transaction: {}", source))]
    ProcessTransaction { source: ProcessTransactionError },
}

#[derive(Clone)]
pub struct Service {
    handle: contagion::ContagionHandle<
        ThinTransaction,
        NetworkSender<ContagionMessage<ThinTransaction>>,
        contagion::Fixed,
    >,
    accounts: Accounts,
    recent_transactions: RecentTransactions,
}

impl Service {
    pub async fn new(
        listener_addr: impl net::ToSocketAddrs + fmt::Display,
        network_keypair: exchange::KeyPair,
        network: Vec<config::Node>,
    ) -> Result<Self, Error> {
        let network_size = network.len();

        let exchanger = Exchanger::new(network_keypair);

        let listener = TcpListener::new(listener_addr, exchanger.clone())
            .await
            .context(ServiceNew)?;

        let connector = ResolveConnector::new(TcpConnector::new(exchanger)).retry();
        // TODO readd connections if dropped
        let mut system = System::new_with_connector_zipped(
            &connector,
            network
                .into_iter()
                .map(|node| (node.public_key, node.address)),
        )
        .await;

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

        let contagion = Contagion::new(
            contagion::Fixed::new_local(),
            ContagionConfig {
                sieve: SieveConfig {
                    sieve_sample_size: network_size,
                    echo_threshold: network_size,
                    murmur: MurmurConfig {
                        murmur_gossip_size: network_size,
                        ..Default::default()
                    },
                },
                contagion_sample_size: network_size,
                ready_threshold: network_size,
            },
        );

        let sampler = AllSampler::default();
        let mut handle = manager.run(contagion, sampler, num_cpus::get()).await;

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
            recent_transactions: RecentTransactions::new(),
        };
        service.spawn();

        Ok(service)
    }

    fn spawn(&self) {
        let mut service = self.clone();

        tokio::spawn(async move {
            let mut to_process = BinaryHeap::new();

            loop {
                match service.handle.deliver().await {
                    Err(contagion::ContagionError::Channel) => break,
                    Err(err) => {
                        warn!("deliver batch: {}", err);
                        continue;
                    }
                    Ok(batch) => {
                        batch.iter().for_each(|msg| {
                            to_process.push(Reverse((
                                (
                                    msg.sequence(),
                                    msg.sender().to_owned(),
                                    msg.payload().to_owned(),
                                ),
                                Instant::now(),
                            )))
                        });
                    }
                };

                // dirty way to process all transactions
                let mut previous_len = usize::MAX;
                while to_process.len() < previous_len {
                    previous_len = to_process.len();

                    let mut remaining_to_process = BinaryHeap::with_capacity(to_process.len());
                    for Reverse((msg, when_added)) in to_process.into_sorted_vec() {
                        if when_added.elapsed() > TRANSACTION_TTL {
                            warn!("dropping too old: {:?}", msg);

                            if let Err(err) = service
                                .recent_transactions
                                .update(Box::new(msg.1), msg.0, TransactionState::Failure)
                                .await
                            {
                                warn!("unable to process: {}", err);
                            }
                        }

                        if let Err(err) = service.process_payload(msg.clone()).await {
                            // retry only account async failures
                            if let ProcessTransactionError::ProcessTxForAccounts {
                                source: accounts::Error::AccountModification { .. },
                            } = err
                            {
                                remaining_to_process.push(Reverse((msg, when_added)));
                            } else {
                                warn!("unable to process: {}", err);
                            }
                        }
                    }
                    to_process = remaining_to_process;
                }
            }
        });
    }

    async fn process_payload(
        &mut self,
        (sequence, sender, payload): (sieve::Sequence, sign::PublicKey, ThinTransaction),
    ) -> Result<(), ProcessTransactionError> {
        info!(sequence, ?sender, tx=?payload, "new payload");

        let sender = Box::new(sender);

        self.accounts
            .transfer(
                sender.clone(),
                sequence,
                Box::new(payload.recipient),
                payload.amount,
            )
            .await
            .context(ProcessTxForAccounts)?;

        self.recent_transactions
            .update(sender, sequence, TransactionState::Success)
            .await
            .context(ProcessTxForRecent)?;

        Ok(())
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
impl From<recent_transactions::Error> for tonic::Status {
    fn from(err: recent_transactions::Error) -> Self {
        Self::invalid_argument(err.to_string())
    }
}

#[tonic::async_trait]
impl at2_server::At2 for Service {
    async fn send_asset(
        &self,
        request: tonic::Request<SendAssetRequest>,
    ) -> Result<tonic::Response<SendAssetReply>, tonic::Status> {
        let message = request.into_inner();

        let thin = at2_node::ThinTransaction {
            recipient: bincode::deserialize(&message.recipient).context(Deserialize)?,
            amount: message.amount,
        };

        let sender = bincode::deserialize(&message.sender).context(Deserialize)?;

        self.recent_transactions
            .put(Box::new(sender), message.sequence, thin.clone())
            .await?;

        self.handle
            .clone()
            .broadcast(&sieve::Payload::new(
                sender,
                message.sequence,
                thin,
                bincode::deserialize(&message.signature).context(Deserialize)?,
            ))
            .await
            .map_err(|err| tonic::Status::invalid_argument(err.to_string()))?;

        Ok(Response::new(SendAssetReply {}))
    }

    async fn get_last_sequence(
        &self,
        request: tonic::Request<GetLastSequenceRequest>,
    ) -> Result<tonic::Response<GetLastSequenceReply>, tonic::Status> {
        let sequence = self
            .accounts
            .get_last_sequence(
                bincode::deserialize(&request.get_ref().sender).context(Deserialize)?,
            )
            .await?;

        Ok(Response::new(GetLastSequenceReply { sequence }))
    }

    async fn get_balance(
        &self,
        request: tonic::Request<GetBalanceRequest>,
    ) -> Result<tonic::Response<GetBalanceReply>, tonic::Status> {
        Ok(Response::new(GetBalanceReply {
            amount: self
                .accounts
                .get_balance(bincode::deserialize(&request.get_ref().sender).context(Deserialize)?)
                .await?,
        }))
    }

    async fn get_latest_transactions(
        &self,
        _: tonic::Request<GetLatestTransactionsRequest>,
    ) -> Result<tonic::Response<GetLatestTransactionsReply>, tonic::Status> {
        use full_transaction::State;

        Ok(Response::new(GetLatestTransactionsReply {
            transactions: self
                .recent_transactions
                .get_all()
                .await?
                .iter()
                .map(|tx| {
                    Ok(proto::FullTransaction {
                        timestamp: tx.timestamp.to_rfc3339(),
                        sender: bincode::serialize(&tx.sender).context(Serialize)?,
                        sender_sequence: tx.sender_sequence,
                        recipient: bincode::serialize(&tx.recipient).context(Serialize)?,
                        amount: tx.amount,
                        state: match tx.state {
                            TransactionState::Pending => State::Pending as i32,
                            TransactionState::Success => State::Success as i32,
                            TransactionState::Failure => State::Failure as i32,
                        },
                    })
                })
                .collect::<Result<_, ProtoError>>()?,
        }))
    }
}
