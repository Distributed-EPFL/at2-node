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

impl Account {
    /// Create a new account
    pub fn new() -> Self {
        Self {
            last_sequence: sieve::Sequence::MIN,
            balance: 10, // TODO create faucet
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
