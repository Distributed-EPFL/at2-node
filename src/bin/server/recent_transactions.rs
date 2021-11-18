use std::collections::VecDeque;

use at2_node::{FullTransaction, ThinTransaction, TransactionState};
use drop::crypto::sign;
use snafu::ensure;
use tokio::sync::{mpsc, oneshot};

const LATEST_TRANSACTIONS_MAX_SIZE: usize = 10;

#[derive(snafu::Snafu, Debug)]
pub enum Error {
    #[snafu(display("gone on send"))]
    GoneOnSend,
    #[snafu(display("gone on recv"))]
    GoneOnRecv,
    PutAlreadyExisting,
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
enum Commands {
    Put {
        sender: Box<sign::PublicKey>,
        sender_sequence: sieve::Sequence,
        thin: ThinTransaction,
        resp: oneshot::Sender<Result<()>>,
    },
    Update {
        sender: Box<sign::PublicKey>,
        sender_sequence: sieve::Sequence,
        state: TransactionState,
        resp: oneshot::Sender<()>,
    },
    GetAll {
        resp: oneshot::Sender<Vec<FullTransaction>>,
    },
}

#[derive(Clone)]
pub struct RecentTransactions {
    agent: mpsc::Sender<Commands>,
}

/// Tokio agent owning the recent transactions.
/// The only way to interacte with it is to use [`RecentTransactions`].
struct RecentTransactionsHandler(VecDeque<FullTransaction>);

impl RecentTransactions {
    pub fn new() -> Self {
        Self {
            agent: RecentTransactionsHandler::new().spawn(),
        }
    }

    /// Add a new transaction
    pub async fn put(
        &self,
        sender: Box<sign::PublicKey>,
        sender_sequence: sieve::Sequence,
        thin: ThinTransaction,
    ) -> Result<()> {
        let (tx, rx) = oneshot::channel();

        self.agent
            .send(Commands::Put {
                sender,
                sender_sequence,
                thin,
                resp: tx,
            })
            .await
            .map_err(|_| Error::GoneOnSend)?;

        rx.await.map_err(|_| Error::GoneOnRecv)?
    }

    /// Update an already put transaction, to resolve its state
    pub async fn update(
        &self,
        sender: Box<sign::PublicKey>,
        sender_sequence: sieve::Sequence,
        state: TransactionState,
    ) -> Result<()> {
        let (tx, rx) = oneshot::channel();

        self.agent
            .send(Commands::Update {
                sender,
                sender_sequence,
                state,
                resp: tx,
            })
            .await
            .map_err(|_| Error::GoneOnSend)?;

        rx.await.map_err(|_| Error::GoneOnRecv)
    }

    /// Return the recently seen transactions
    pub async fn get_all(&self) -> Result<Vec<FullTransaction>> {
        let (tx, rx) = oneshot::channel();

        self.agent
            .send(Commands::GetAll { resp: tx })
            .await
            .map_err(|_| Error::GoneOnSend)?;

        rx.await.map_err(|_| Error::GoneOnRecv)
    }
}

impl RecentTransactionsHandler {
    fn new() -> Self {
        Self(VecDeque::with_capacity(LATEST_TRANSACTIONS_MAX_SIZE))
    }

    fn spawn(mut self) -> mpsc::Sender<Commands> {
        let (tx, mut rx) = mpsc::channel(32);

        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Commands::Put {
                        sender,
                        sender_sequence,
                        thin,
                        resp,
                    } => {
                        let _ = resp.send(self.put(*sender, sender_sequence, thin));
                    }
                    Commands::Update {
                        sender,
                        sender_sequence,
                        state,
                        resp,
                    } => {
                        self.update(*sender, sender_sequence, state);
                        let _ = resp.send(());
                    }
                    Commands::GetAll { resp } => {
                        let _ = resp.send(self.get_all());
                    }
                }
            }
        });

        tx
    }

    fn put(
        &mut self,
        sender: sign::PublicKey,
        sender_sequence: sieve::Sequence,
        thin: ThinTransaction,
    ) -> Result<()> {
        ensure!(
            !self
                .0
                .iter()
                .any(|tx| tx.sender_sequence == sender_sequence && tx.sender == sender),
            PutAlreadyExisting
        );

        let full = FullTransaction {
            timestamp: chrono::Utc::now(),
            sender,
            sender_sequence,
            recipient: thin.recipient,
            amount: thin.amount,
            state: TransactionState::Pending,
        };

        if self.0.len() == LATEST_TRANSACTIONS_MAX_SIZE {
            self.0.pop_front();
        }

        self.0.push_back(full);

        Ok(())
    }

    fn update(
        &mut self,
        sender: sign::PublicKey,
        sender_sequence: sieve::Sequence,
        state: TransactionState,
    ) {
        // NOP if not found as the transaction may resolve late
        if let Some(tx) = self
            .0
            .iter_mut()
            .find(|tx| tx.sender_sequence == sender_sequence && tx.sender == sender)
        {
            tx.state = state;
        }
    }

    fn get_all(&self) -> Vec<FullTransaction> {
        self.0.clone().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_transactions_show_in_get_all() {
        let recent_transactions = RecentTransactions::new();

        let sender = sign::KeyPair::random().public();
        let recipient = sign::KeyPair::random().public();

        let txs = [
            ThinTransaction {
                amount: 10,
                recipient,
            },
            ThinTransaction {
                amount: 3,
                recipient: sender,
            },
        ];

        for (tx, seq) in txs.iter().zip(1..) {
            recent_transactions
                .put(Box::new(sender), seq, tx.clone())
                .await
                .expect("to put transaction");
        }

        let recent_txs = recent_transactions
            .get_all()
            .await
            .expect("to get recent txs");

        assert_eq!(txs.len(), recent_txs.len());
        txs.iter()
            .zip(recent_txs.iter())
            .zip(1..)
            .for_each(|((thin, full), seq)| {
                assert_eq!(sender, full.sender);
                assert_eq!(seq, full.sender_sequence);
                assert_eq!(thin.amount, full.amount);
                assert_eq!(thin.recipient, full.recipient);
                assert_eq!(TransactionState::Pending, full.state);
            });
    }
}
