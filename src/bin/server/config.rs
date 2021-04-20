use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct Config {
    pub address: SocketAddr,
}
