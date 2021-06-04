use snafu::{ensure, OptionExt};

#[derive(snafu::Snafu, Debug)]
pub enum Error {
    InconsecutiveSequence,
    Overflow,
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

    pub fn credit(&self, sequence: sieve::Sequence, amount: u64) -> Result<Self, Error> {
        self.mutate_balance(u64::checked_add, sequence, amount)
    }

    pub fn debit(&self, sequence: sieve::Sequence, amount: u64) -> Result<Self, Error> {
        self.mutate_balance(u64::checked_sub, sequence, amount)
    }

    fn mutate_balance<F>(
        &self,
        mutator: F,
        sequence: sieve::Sequence,
        amount: u64,
    ) -> Result<Self, Error>
    where
        F: FnOnce(u64, u64) -> Option<u64>,
    {
        ensure!(self.last_sequence + 1 != sequence, InconsecutiveSequence);

        let new_balance = mutator(self.balance, amount).context(Overflow)?;

        Ok(Self {
            last_sequence: sequence,
            balance: new_balance,
        })
    }
}
