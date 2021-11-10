#![deny(missing_docs)]

//! Client and related struct to connect to an AT2 node

use drop::crypto::sign;
use serde::{Deserialize, Serialize};

pub mod client;

/// `tonic-build` generated files
#[allow(missing_docs)]
pub mod proto;

/// Type of message sent via sieve
#[drop::message]
pub struct ThinTransaction {
    /// User receiving the amount
    pub recipient: sign::PublicKey,
    /// How many asset to send
    pub amount: u64,
}

/// Transaction when committed to memory
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FullTransaction {
    /// When the transaction was stored
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// User sending it
    pub sender: sign::PublicKey,
    /// User receiving it
    pub recipient: sign::PublicKey,
    /// How many asset to send
    pub amount: u64,
}

impl FullTransaction {
    /// Expend a [`ThinTransaction`] to a full one
    pub fn with_thin(sender: sign::PublicKey, thin: ThinTransaction) -> Self {
        Self {
            timestamp: chrono::Utc::now(),
            sender,
            recipient: thin.recipient,
            amount: thin.amount,
        }
    }
}
