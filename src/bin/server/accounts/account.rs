use snafu::{ensure, OptionExt};

#[derive(snafu::Snafu, Debug)]
pub enum Error {
    InconsecutiveSequence,
    Overflow,
    Underflow,
}

pub struct Account {
    last_sequence: sieve::Sequence,
    balance: u64,
}

impl Account {
    pub fn new() -> Self {
        Self {
            last_sequence: sieve::Sequence::MIN,
            balance: 10, // TODO create faucet
        }
    }

    pub fn credit(&self, amount: u64) -> Result<Self, Error> {
        Ok(Self {
            last_sequence: self.last_sequence,
            balance: self.balance.checked_add(amount).context(Overflow)?,
        })
    }

    pub fn debit(&self, sequence: sieve::Sequence, amount: u64) -> Result<Self, Error> {
        ensure!(self.last_sequence + 1 != sequence, InconsecutiveSequence);

        Ok(Self {
            last_sequence: sequence,
            balance: self.balance.checked_sub(amount).context(Underflow)?,
        })
    }

    pub fn balance(&self) -> u64 {
        self.balance
    }
}
