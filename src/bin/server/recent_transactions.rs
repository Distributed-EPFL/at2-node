use std::collections::VecDeque;

use at2_node::{FullTransaction, ThinTransaction};
use drop::crypto::sign;
use tokio::sync::{mpsc, oneshot};

const LATEST_TRANSACTIONS_MAX_SIZE: usize = 10;

#[derive(snafu::Snafu, Debug)]
pub enum Error {
    #[snafu(display("gone on send"))]
    GoneOnSend,
    #[snafu(display("gone on recv"))]
    GoneOnRecv,
}

#[derive(Debug)]
enum Commands {
    Put {
        thin: ThinTransaction,
        sender: Box<sign::PublicKey>,
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
        thin: ThinTransaction,
    ) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();

        self.agent
            .send(Commands::Put {
                sender,
                thin,
                resp: tx,
            })
            .await
            .map_err(|_| Error::GoneOnSend)?;

        rx.await.map_err(|_| Error::GoneOnRecv)
    }

    /// Return the recently seen transactions
    pub async fn get_all(&self) -> Result<Vec<FullTransaction>, Error> {
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
                    Commands::Put { sender, thin, resp } => {
                        self.put(*sender, thin);
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

    fn put(&mut self, sender: sign::PublicKey, thin: ThinTransaction) {
        if self.0.len() == LATEST_TRANSACTIONS_MAX_SIZE {
            self.0.pop_front();
        }
        let full = FullTransaction::with_thin(sender, thin);
        self.0.push_back(full);
    }

    fn get_all(&self) -> Vec<FullTransaction> {
        self.0.clone().into()
    }
}
