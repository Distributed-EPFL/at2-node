#[cfg(all(test, not(all(feature = "server", feature = "client"))))]
compile_error!("tests need both server and client features");

use std::{
    io,
    io::{BufRead, BufReader},
    iter::{repeat_with, Extend},
    net::{Ipv4Addr, SocketAddr},
    sync::{
        atomic::{AtomicU16, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use duct::cmd;
use futures::future::join_all;
use tokio::{net::TcpStream, task::yield_now};
use tracing::info;
use tracing_subscriber;
use structopt::StructOpt;
use rand::{thread_rng, Rng};

const CLIENT_BIN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/debug/client");
const SERVER_BIN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/debug/server");

const TICK: Duration = Duration::from_millis(100);
const TIMEOUT: Duration = Duration::from_secs(10);

fn next_test_port() -> u16 {
    static PORT_OFFSET: AtomicU16 = AtomicU16::new(0);
    const PORT_START: u16 = 3000;

    PORT_START + PORT_OFFSET.fetch_add(1, Ordering::Relaxed)
}

fn next_test_ip4() -> SocketAddr {
    (Ipv4Addr::new(127, 0, 0, 1), next_test_port()).into()
}

struct Server {
    handle: Arc<duct::ReaderHandle>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        use std::thread;

        use nix::{
            sys::signal::{self, Signal},
            unistd::Pid,
        };

        self.handle.pids().iter().for_each(|pid| {
            let _ = signal::kill(Pid::from_raw(*pid as i32), Signal::SIGTERM);
        });

        let timeout = Instant::now();
        while Instant::now() < timeout {
            if let Ok(None) = self.handle.try_wait() {
                thread::sleep(TICK);
            }
        }

        self.handle.kill().expect("kill server");

        std::mem::take(&mut self.reader)
            .map(|reader| reader.join().expect("finish reader"))
            .unwrap();
    }
}

type ServerConfig = Vec<u8>;
type NodeConfig = Vec<u8>;

fn gen_config(node: &SocketAddr, rpc: &SocketAddr) -> (ServerConfig, NodeConfig) {
    let full_config = cmd!(
        SERVER_BIN,
        "config",
        "new",
        &node.to_string(),
        &rpc.to_string()
    )
    .stdout_capture()
    .run()
    .expect("generate config")
    .stdout;

    let node_config = cmd!(SERVER_BIN, "config", "get-node")
        .stdin_bytes(full_config.clone())
        .stdout_capture()
        .run()
        .expect("get node config")
        .stdout;

    (full_config, node_config)
}

fn start_server(server_config: ServerConfig) -> Server {
    let handle = cmd!(SERVER_BIN, "run")
        .stdin_bytes(server_config)
        .stderr_to_stdout()
        .reader()
        .map(Arc::new)
        .expect("run server");

    let to_read = handle.clone();
    Server {
        handle,
        reader: Some(std::thread::spawn(move || {
            let mut reader = BufReader::new(&*to_read);
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok() {
                print!("{}", line);
                line.clear();
            }
        })),
    }
}

async fn wait_until_connect(server: &Server, to_probe: &SocketAddr) {
    while let Err(err) = TcpStream::connect(to_probe).await {
        if err.kind() != io::ErrorKind::ConnectionRefused {
            panic!("connect server: {}", err)
        }

        if let Err(err) = server.handle.try_wait() {
            panic!("server finished early: {}", err);
        }

        yield_now().await;
    }
}

async fn start_network(size: usize) -> (Vec<Server>, Vec<(SocketAddr, SocketAddr)>) {
    let addresses = repeat_with(|| (next_test_ip4(), next_test_ip4()))
        .take(size)
        .collect::<Vec<_>>();

    let (mut server_configs, node_configs): (Vec<_>, Vec<_>) = addresses
        .iter()
        .map(|(node, rpc)| gen_config(node, rpc))
        .unzip();

    server_configs
        .iter_mut()
        .enumerate()
        .for_each(|(server_pos, server_config)| {
            server_config.extend(
                node_configs
                    .iter()
                    .enumerate()
                    .filter(|(node_pos, _)| *node_pos != server_pos)
                    .map(|(_, node_config)| node_config)
                    .flatten(),
            )
        });

    let servers: Vec<_> = server_configs
        .iter()
        .map(|server_config| start_server(server_config.clone()))
        .collect();

    join_all(servers.iter().zip(&addresses).flat_map(|(server, addrs)| {
        vec![
            wait_until_connect(server, &addrs.0),
            wait_until_connect(server, &addrs.1),
        ]
    }))
    .await;

    // let rpc = addresses
    //     .iter()
    //     .map(|(_, rpc)| rpc)
    //     .copied()
    //     .next()
    //     .map(|addr| Url::parse(&format!("http://{}", addr)).expect("format as URL"))
    //     .expect("zero sized network");

    (servers, addresses)
}

fn transfer(
    sender_config: &String,
    sender_sequence: sieve::Sequence,
    receiver_config: &String,
    amount: usize,
) {
    info!("sender-config: {}", sender_config);
    info!("receiver-config: {}", receiver_config);

    let second_client = cmd!(CLIENT_BIN, "config", "get-public-key")
        .stdin_bytes(receiver_config.clone())
        .read()
        .expect("get public key");

    cmd!(
        CLIENT_BIN,
        "send-asset",
        &sender_sequence.to_string(),
        &second_client,
        &amount.to_string(),
    )
    .stdin_bytes(sender_config.clone())
    .run()
    .expect("send asset");
}

fn get_last_sequence(config: String) -> sieve::Sequence {
    cmd!(CLIENT_BIN, "get-last-sequence")
        .stdin_bytes(config)
        .read()
        .expect("get last sequence")
        .parse::<sieve::Sequence>()
        .expect("parse as Sequence")
}

async fn wait_for_sequence(config: &String, sequence: sieve::Sequence) {
    let timeout = Instant::now() + TIMEOUT;
    while Instant::now() < timeout {
        let last_sequence = get_last_sequence(config.clone());
        if last_sequence == sequence {
            return;
        }
        else {
            info!("error: last_sequence={} != sequence={}", last_sequence, sequence);
        }
        tokio::time::sleep(TICK).await;
    }

    panic!("timeout expired");
}

fn get_balance(config: &String) -> usize {
    cmd!(CLIENT_BIN, "get-balance")
        .stdin_bytes(config.clone())
        .read()
        .expect("get asset")
        .parse::<usize>()
        .expect("parse asset amount as usize")
}

fn shutdown_server(servers: Vec<Server>) {
    for s in servers {
        drop(s);
    }
}

fn to_http(socket: &SocketAddr) -> String {
    "http://".to_owned() + &socket.to_string() +"/"
}

fn get_account() -> (String, sieve::Sequence) {
    (
        cmd!(CLIENT_BIN, "config", "new", "dummy_addr")
            .read()
            .expect("create new account"),
        1,
    )
}

fn set_address_str(buffer: &mut String, new_address: &String) {
    let first_line = buffer.find("\n").unwrap_or(buffer.len());
    let new_line: String = "rpc_address = \"".to_owned() + new_address + "\"";
    buffer.replace_range(..first_line, &new_line);
}

fn set_address(buffer: &mut String, socket: &SocketAddr) {
    set_address_str(buffer, &to_http(socket));
}

fn get_two_random(n: usize) -> (usize, usize) {
    let mut rng = thread_rng();
    let ni: usize = rng.gen_range(0..n);
    let nk: usize = if ni > n/2 { ni - n/4 } else { ni + n/4 };
    (ni, nk)
}

async fn generate_transfer(
    nodes: &Vec<(SocketAddr, SocketAddr)>,
    accounts: &mut Vec<(String, sieve::Sequence)>,
) {
    let (node_i, node_k) = get_two_random(nodes.len());
    let (sender_acc, receiver_acc) = get_two_random(accounts.len());

    // Sender and Receiver are connecting themselves
    // to 2 different nodes in the network
    set_address(&mut accounts[sender_acc].0, &nodes[node_i].1);
    set_address(&mut accounts[receiver_acc].0, &nodes[node_k].1);

    info!("Out of a network of {} nodes: node_i={} node_k={}", nodes.len(), node_i, node_k);
    info!("Account {} is sending to account {}", sender_acc, receiver_acc);

    let (sender, sender_sequence) = &accounts[sender_acc];
    let (receiver, _) = &accounts[receiver_acc];

    let amount = if get_balance(&sender) > 0 { 1 } else { 0 };

    let bal_before_sender = get_balance(&sender);
    let bal_before_receiver = get_balance(&receiver);

    transfer(&sender, *sender_sequence, &receiver, amount);
    wait_for_sequence(&sender, *sender_sequence).await;

    let bal_after_sender = get_balance(&sender);
    let bal_after_receiver = get_balance(&receiver);

    // bump sequence number
    accounts[sender_acc].1 = sender_sequence + 1;

    assert!(bal_before_sender - amount == bal_after_sender,
            "sender: {} - {} != {}", bal_before_sender, amount, bal_after_sender);

    assert!(bal_before_receiver + amount == bal_after_receiver,
            "receiver: {} + {} != {}", bal_before_receiver, amount, bal_after_receiver);
}

async fn run_simulation(network_size: usize, number_of_account: usize, number_of_transfer: usize) {
    info!("Launching network of {} nodes", network_size);
    let (instances, network) = start_network(network_size).await;
    info!("Network of {} nodes launched", network_size);

    for (addr, rpc) in &network {
        info!("addr={:?} rpc={:?}", to_http(addr), to_http(rpc));
    }

    // Generate `number_of_account` accounts (private key; rpc address to node)
    let mut accounts = Vec::with_capacity(number_of_account);
    for _ in 0..number_of_account {
        accounts.push(get_account())
    }

    for i in 1..=number_of_transfer {
        generate_transfer(&network, &mut accounts).await;
        info!("Transfer {}/{} done", i, number_of_transfer);
    }

    info!("Shutting down the network");
    shutdown_server(instances);
}

#[derive(Debug, StructOpt)]
struct Cli {
    #[structopt(short = "n", long = "network-size", default_value = "4")]
    network_size: usize,

    #[allow(dead_code)]
    #[structopt(short = "s", long = "sample-size", default_value = "2")]
    sample_size: usize, // Should work with O(log(network_size))

    #[structopt(short = "t", long = "transfer", default_value = "10")]
    number_of_transfer: usize,

    #[structopt(short = "a", long = "accounts", default_value = "10")]
    number_of_account: usize,
}

#[tokio::main(worker_threads = 1)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let args = Cli::from_args();

    run_simulation(args.network_size, args.number_of_account, args.number_of_transfer).await;

    Ok(())
}
