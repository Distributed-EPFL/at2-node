use snafu::{ensure, OptionExt};

#[derive(snafu::Snafu, Debug)]
pub enum Error {
    InconsecutiveSequence,
    Overflow,
    Underflow,
}

/// Contains the balance for a user
#[derive(Debug)]
pub struct Account {
    last_sequence: sieve::Sequence,
    balance: u64,
}

const INITIAL_BALANCE: u64 = 10;

impl Account {
    /// Create a new account
    pub fn new() -> Self {
        Self {
            last_sequence: sieve::Sequence::MIN,
            balance: INITIAL_BALANCE, // TODO create faucet
        }
    }

    /// Add some amount to this account
    pub fn credit(&self, amount: u64) -> Result<Self, Error> {
        Ok(Self {
            last_sequence: self.last_sequence,
            balance: self.balance.checked_add(amount).context(Overflow)?,
        })
    }

    /// Remove some amount from this account, iff the `sequence` is consecutive to the last one
    pub fn debit(&self, sequence: sieve::Sequence, amount: u64) -> Result<Self, Error> {
        ensure!(self.last_sequence + 1 == sequence, InconsecutiveSequence);

        Ok(Self {
            last_sequence: sequence,
            balance: self.balance.checked_sub(amount).context(Underflow)?,
        })
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
        let account = Account::new();

        account
            .debit(1, INITIAL_BALANCE + 1)
            .expect_err("able to debit more than possessed");
    }

    #[test]
    fn debit_increase_sequence() {
        let account = Account::new();

        let new_accout = account.debit(1, 1).expect("to debit account");

        assert!(account.last_sequence() < new_accout.last_sequence());
    }

    #[test]
    fn credit_doesnt_change_sequence() {
        let account = Account::new();

        let new_accout = account.credit(1).expect("to credit account");

        assert_eq!(account.last_sequence(), new_accout.last_sequence());
    }
}
