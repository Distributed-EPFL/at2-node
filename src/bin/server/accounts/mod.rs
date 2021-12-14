use std::collections::HashMap;

use drop::crypto::sign;
use snafu::ResultExt;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};

pub mod account;
use account::Account;

#[derive(snafu::Snafu, Debug)]
pub enum Error {
    NoSuchAccount {
        pubkey: Box<sign::PublicKey>,
    },
    AccountModification {
        source: account::Error,
    },

    #[snafu(display("gone on send"))]
    GoneOnSend,
    #[snafu(display("gone on recv"))]
    GoneOnRecv,
}

type Response<T> = oneshot::Sender<Result<T, Error>>;

#[derive(Debug)]
enum Commands {
    GetBalance {
        user: Box<sign::PublicKey>,
        resp: Response<u64>,
    },
    GetLastSequence {
        user: Box<sign::PublicKey>,
        resp: oneshot::Sender<sieve::Sequence>,
    },
    Transfer {
        sender: Box<sign::PublicKey>,
        sender_sequence: sieve::Sequence,
        receiver: Box<sign::PublicKey>,
        amount: u64,
        resp: Response<()>,
    },
}

#[derive(Clone)]
pub struct Accounts {
    agent: mpsc::Sender<Commands>,
}

/// Own the accounts themselves
struct AccountsHandler {
    ledger: HashMap<sign::PublicKey, account::Account>,
}

impl Accounts {
    pub fn new() -> Self {
        Self {
            agent: AccountsHandler::new().spawn(),
        }
    }

    /// Return the balance for the given user
    pub async fn get_balance(&self, user: Box<sign::PublicKey>) -> Result<u64, Error> {
        let (tx, rx) = oneshot::channel();

        self.agent
            .send(Commands::GetBalance { user, resp: tx })
            .await
            .map_err(|_| Error::GoneOnSend)?;

        rx.await.map_err(|_| Error::GoneOnRecv)?
    }

    /// Transfer an `amount` from the `sender` account to the `receiver`
    ///
    /// It fails if the `sender_sequence` is not consecutive to the last one transfered
    /// transaction.
    pub async fn transfer(
        &self,
        sender: Box<sign::PublicKey>,
        sender_sequence: sieve::Sequence,
        receiver: Box<sign::PublicKey>,
        amount: u64,
    ) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();

        self.agent
            .send(Commands::Transfer {
                sender,
                sender_sequence,
                receiver,
                amount,
                resp: tx,
            })
            .await
            .map_err(|_| Error::GoneOnSend)?;

        rx.await.map_err(|_| Error::GoneOnRecv)?
    }

    /// Return the last sequence used for this user.
    pub async fn get_last_sequence(
        &self,
        user: Box<sign::PublicKey>,
    ) -> Result<sieve::Sequence, Error> {
        let (tx, rx) = oneshot::channel();

        self.agent
            .send(Commands::GetLastSequence { user, resp: tx })
            .await
            .map_err(|_| Error::GoneOnSend)?;

        Ok(rx.await.map_err(|_| Error::GoneOnRecv)?)
    }
}

impl AccountsHandler {
    fn new() -> Self {
        Self {
            ledger: Default::default(),
        }
    }

    fn spawn(mut self) -> mpsc::Sender<Commands> {
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
                        let _ =
                            resp.send(self.transfer(*sender, sender_sequence, *receiver, amount));
                    }
                    Commands::GetLastSequence { user, resp } => {
                        let _ = resp.send(self.get_last_sequence(*user));
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

        if sender.eq(&receiver) {
            warn!(?sender, "transfer to itself");

            let account = self.ledger.entry(sender).or_insert(initial_account);

            account
                .debit(sender_sequence, 0)
                .context(AccountModification)?;
        } else {
            let mut sender_account = *self.ledger.get(&sender).unwrap_or(&initial_account);
            let mut receiver_account = *self.ledger.get(&receiver).unwrap_or(&initial_account);

            debug!(?sender_account, ?receiver_account, "before transfer");

            let sender_res = sender_account
                .debit(sender_sequence, amount)
                .context(AccountModification);
            self.ledger.insert(sender, sender_account);

            sender_res?;

            receiver_account
                .credit(amount)
                .context(AccountModification)?;
            self.ledger.insert(receiver, receiver_account);

            info!(?sender_account, ?receiver_account, "after transfer");
        }

        Ok(())
    }

    fn get_last_sequence(&self, sender: sign::PublicKey) -> sieve::Sequence {
        if let Some(sender_account) = self.ledger.get(&sender) {
            sender_account.last_sequence()
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn get_balance_and_sequence(
        accounts: &Accounts,
        user_pubkey: Box<sign::PublicKey>,
    ) -> (u64, sieve::Sequence) {
        (
            accounts
                .get_balance(user_pubkey.clone())
                .await
                .expect("to get balance"),
            accounts
                .get_last_sequence(user_pubkey)
                .await
                .expect("to get last sequence"),
        )
    }

    #[tokio::test]
    async fn new_account_is_the_same_as_unknown_account() {
        let accounts = Accounts::new();
        let user_pubkey = Box::new(sign::KeyPair::random().public());

        let new_account = Account::new();

        let (balance, sequence) = get_balance_and_sequence(&accounts, user_pubkey).await;

        assert_eq!(balance, new_account.balance(),);
        assert_eq!(sequence, new_account.last_sequence(),);
    }

    #[tokio::test]
    async fn transfer_to_themselves_increment_sequence_and_keep_balance() {
        let accounts = Accounts::new();
        let user_pubkey = Box::new(sign::KeyPair::random().public());

        let (initial_balance, initial_sequence) =
            get_balance_and_sequence(&accounts, user_pubkey.clone()).await;

        accounts
            .transfer(user_pubkey.clone(), 1, user_pubkey.clone(), 10)
            .await
            .expect("to transfer to themselves");

        let (final_balance, final_sequence) =
            get_balance_and_sequence(&accounts, user_pubkey).await;

        assert_eq!(initial_balance, final_balance);
        assert!(initial_sequence < final_sequence);
    }

    #[tokio::test]
    async fn transfer_too_much_fails_and_increases_sequence() {
        let accounts = Accounts::new();
        let first_user_pubkey = Box::new(sign::KeyPair::random().public());
        let second_user_pubkey = Box::new(sign::KeyPair::random().public());

        let (first_initial_balance, first_initial_sequence) =
            get_balance_and_sequence(&accounts, first_user_pubkey.clone()).await;
        let (second_initial_balance, second_initial_sequence) =
            get_balance_and_sequence(&accounts, second_user_pubkey.clone()).await;

        accounts
            .transfer(
                first_user_pubkey.clone(),
                1,
                second_user_pubkey.clone(),
                first_initial_balance + 1,
            )
            .await
            .expect_err("to fail to transfer");

        let (first_final_balance, first_final_sequence) =
            get_balance_and_sequence(&accounts, first_user_pubkey).await;
        let (second_final_balance, second_final_sequence) =
            get_balance_and_sequence(&accounts, second_user_pubkey).await;

        assert_eq!(first_initial_balance, first_final_balance);
        assert!(first_initial_sequence < first_final_sequence,);

        assert_eq!(second_initial_balance, second_final_balance);
        assert_eq!(second_initial_sequence, second_final_sequence,);
    }
}
