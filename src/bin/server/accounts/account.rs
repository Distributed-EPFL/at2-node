use snafu::{ensure, OptionExt};

#[derive(snafu::Snafu, Debug)]
pub enum Error {
    InconsecutiveSequence,
    Overflow,
    Underflow,
}

/// Contains the balance and the latest processed sequence for a user
#[derive(Debug, Clone, Copy)]
pub struct Account {
    last_sequence: sieve::Sequence,
    balance: u64,
}

const INITIAL_BALANCE: u64 = 100000;

impl Account {
    /// Create a new account
    pub fn new() -> Self {
        Self {
            last_sequence: sieve::Sequence::MIN,
            balance: INITIAL_BALANCE, // TODO create faucet
        }
    }

    /// Add some amount to this account
    pub fn credit(&mut self, amount: u64) -> Result<(), Error> {
        self.balance = self.balance.checked_add(amount).context(Overflow)?;

        Ok(())
    }

    /// Remove some amount from this account, iff the `sequence` is consecutive to the last one
    pub fn debit(&mut self, sequence: sieve::Sequence, amount: u64) -> Result<(), Error> {
        ensure!(self.last_sequence + 1 == sequence, InconsecutiveSequence);
        self.last_sequence = sequence;

        self.balance = self.balance.checked_sub(amount).context(Underflow)?;

        Ok(())
    }

    /// Return the last used sequence
    pub fn last_sequence(&self) -> sieve::Sequence {
        self.last_sequence
    }

    /// Return the owned amount
    pub fn balance(&self) -> u64 {
        self.balance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debit_too_much_fails() {
        let mut account = Account::new();

        let old_seq = account.last_sequence();
        account
            .debit(1, INITIAL_BALANCE + 1)
            .expect_err("able to debit more than possessed");

        assert!(old_seq < account.last_sequence());
    }

    #[test]
    fn debit_increase_sequence() {
        let mut account = Account::new();

        let old_seq = account.last_sequence();
        account.debit(1, 1).expect("to debit account");

        assert!(old_seq < account.last_sequence());
    }

    #[test]
    fn credit_doesnt_change_sequence() {
        let mut account = Account::new();

        let old_seq = account.last_sequence();
        account.credit(1).expect("to credit account");

        assert_eq!(old_seq, account.last_sequence());
    }
}
