use drop::crypto::sign;
use serde::{Deserialize, Serialize};

pub mod client;
pub mod proto;

#[drop::message]
pub struct ThinTransaction {
    pub recipient: sign::PublicKey,
    pub amount: u64,
}

#[derive(Debug, Clone)]
pub struct FullTransaction {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub sender: sign::PublicKey,
    pub recipient: sign::PublicKey,
    pub amount: u64,
}

impl FullTransaction {
    pub fn with_thin(sender: sign::PublicKey, thin: ThinTransaction) -> Self {
        Self {
            timestamp: chrono::Utc::now(),
            sender,
            recipient: thin.recipient,
            amount: thin.amount,
        }
    }
}
