use serde::{Deserialize, Serialize};

use drop::crypto::sign;

pub mod proto;

#[drop::system::message]
pub struct Transaction {
    pub recipient: sign::PublicKey,
    pub amount: u64,
}
