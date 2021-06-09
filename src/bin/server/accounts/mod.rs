use std::collections::HashMap;

use snafu::ResultExt;
use tokio::sync::{mpsc, oneshot};

use drop::crypto::sign;

mod account;
use account::Account;

#[derive(snafu::Snafu, Debug)]
pub enum Error {
    NoSuchAccount { pubkey: sign::PublicKey },
    AccountModification { source: account::Error },
}

type Response<T> = oneshot::Sender<Result<T, Error>>;

#[derive(Debug)]
pub enum Commands {
    GetBalance {
        user: sign::PublicKey,
        resp: Response<u64>,
    },
    Transfer {
        sender: sign::PublicKey,
        sender_sequence: sieve::Sequence,
        receiver: sign::PublicKey,
        amount: u64,
        resp: Response<()>,
    },
}

pub struct Accounts {
    ledger: HashMap<sign::PublicKey, account::Account>,
}

// TODO do not expose channels but use `async fn get_balance(user) -> Result<u64, Error>`
impl Accounts {
    pub fn new() -> Self {
        Self {
            ledger: Default::default(),
        }
    }

    pub fn spawn(mut self) -> mpsc::Sender<Commands> {
        let (tx, mut rx) = mpsc::channel(32);

        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Commands::GetBalance { user, resp } => {
                        let _ = resp.send(Ok(self.get_balance(&user)));
                    }
                    Commands::Transfer {
                        sender,
                        sender_sequence,
                        receiver,
                        amount,
                        resp,
                    } => {
                        let _ = resp.send(self.transfer(sender, sender_sequence, receiver, amount));
                    }
                }
            }
        });

        tx
    }

    fn get_balance(&self, user: &sign::PublicKey) -> u64 {
        // TODO remove me when create_account is done
        let initial_account = Account::new();

        self.ledger
            .get(user)
            .map(|account| account.balance())
            .unwrap_or_else(|| initial_account.balance())
    }

    fn transfer(
        &mut self,
        sender: sign::PublicKey,
        sender_sequence: sieve::Sequence,
        receiver: sign::PublicKey,
        amount: u64,
    ) -> Result<(), Error> {
        // TODO remove me when create_account is done
        let initial_account = Account::new();

        let sender_account = self.ledger.get(&sender).unwrap_or(&initial_account);
        let receiver_account = self.ledger.get(&receiver).unwrap_or(&initial_account);

        let new_sender_account = sender_account
            .debit(sender_sequence, amount)
            .context(AccountModification)?;
        let new_receiver_account = receiver_account
            .credit(amount)
            .context(AccountModification)?;

        self.ledger.insert(sender, new_sender_account);
        self.ledger.insert(receiver, new_receiver_account);

        Ok(())
    }
}
