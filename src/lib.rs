use drop::crypto::sign;
use serde::{Serialize, Deserialize};

pub mod client;
pub mod proto;

#[drop::message]
pub struct Transaction {
    pub recipient: sign::PublicKey,
    pub amount: u64,
}
