use std::net::SocketAddr;

use drop::crypto::key::exchange::{self, Exchanger};
use drop::crypto::sign;
use drop::net::ListenerError;
use drop::net::{ConnectorExt, TcpConnector, TcpListener};
use drop::system::{AllSampler, Handle, NetworkSender, System, SystemManager};
use murmur::MurmurConfig;
use sieve::{self, Sieve, SieveConfig, SieveMessage};

use tonic::Response;

tonic::include_proto!("at2");

use at2_server::At2;
pub use at2_server::At2Server;

type M = u64;

pub struct Service {
    handle: sieve::SieveHandle<M, NetworkSender<SieveMessage<M>>, sieve::Fixed>,
}

use super::config;

impl Service {
    pub async fn new(
        listener_addr: SocketAddr,
        network_keypair: exchange::KeyPair,
        sign_keypair: sign::KeyPair,
        network: Vec<config::Node>,
    ) -> Result<Self, ListenerError> {
        let exchanger = Exchanger::new(network_keypair);

        let listener = TcpListener::new(listener_addr, exchanger.clone()).await?;

        let (addrs, keys): (Vec<_>, Vec<_>) = network
            .iter()
            .map(|node| (node.address, node.public_key))
            .unzip();
        let connector = TcpConnector::new(exchanger).retry();
        // TODO readd connections if dropped
        let mut system = System::new_with_connector(&connector, keys, addrs).await;

        let _ = system.add_listener(listener).await;
        let manager = SystemManager::new(system);

        let sieve = Sieve::new(
            sign_keypair,
            sieve::Fixed::new_local(),
            SieveConfig {
                sieve_sample_size: 1,
                murmur: MurmurConfig {
                    murmur_gossip_size: network.len(),
                    ..Default::default()
                },
                ..Default::default()
            },
        );

        let sampler = AllSampler::default();

        // TODO log errors from manager
        Ok(Self {
            handle: manager
                .run(sieve, sampler, num_cpus::get())
                .await
                .processor_handle(),
        })
    }
}

#[tonic::async_trait]
impl At2 for Service {
    async fn send_money(
        &self,
        _request: tonic::Request<SendMoneyRequest>,
    ) -> Result<tonic::Response<SendMoneyReply>, tonic::Status> {
        // TODO by the client
        let sign_keypair = sign::KeyPair::random();
        let mut signer = sign::Signer::new(sign_keypair.clone());
        let message: u64 = 0;
        let signature = signer.sign(&message).expect("sign failed");
        let payload = sieve::Payload::new(*sign_keypair.public(), 0, message, signature);

        self.handle
            .clone()
            .broadcast(&payload)
            .await
            .expect("broadcasting failed");

        let reply = SendMoneyReply { request_id: vec![] };
        Ok(Response::new(reply))
    }
}
