#[cfg(all(test, not(all(feature = "server", feature = "client"))))]
compile_error!("tests need both server and client features");

use std::{
    env, io,
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
use url::Url;

const CLIENT_BIN: &str = env!("CARGO_BIN_EXE_client");
const SERVER_BIN: &str = env!("CARGO_BIN_EXE_server");

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

        let timeout = Instant::now() + TIMEOUT;
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

#[tokio::test]
async fn server_started_twice_fails() {
    let (node, rpc) = (next_test_ip4(), next_test_ip4());

    let (server_config, _) = gen_config(&node, &rpc);

    let first_server = start_server(server_config.clone());
    join_all(vec![
        wait_until_connect(&first_server, &node),
        wait_until_connect(&first_server, &rpc),
    ])
    .await;

    let second_server = start_server(server_config);

    let timeout = Instant::now() + TIMEOUT;
    let mut exit = None;
    while Instant::now() < timeout {
        if let Err(err) = second_server.handle.try_wait() {
            exit = Some(err.kind());
            break;
        }

        tokio::time::sleep(TICK).await;
    }

    assert_eq!(exit, Some(io::ErrorKind::Other));
}

async fn start_network(size: usize) -> (Vec<Server>, Url) {
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

    let rpc = addresses
        .iter()
        .map(|(_, rpc)| rpc)
        .copied()
        .next()
        .map(|addr| Url::parse(&format!("http://{}", addr)).expect("format as URL"))
        .expect("zero sized network");

    (servers, rpc)
}

#[tokio::test]
async fn can_run_network() {
    start_network(3).await;
}

#[tokio::test]
async fn client_without_servers_fails() {
    let (_, rpc) = start_network(2).await;

    let recipient = cmd!(CLIENT_BIN, "config", "new", &rpc.to_string())
        .pipe(cmd!(CLIENT_BIN, "config", "get-public-key"))
        .read()
        .expect("recipient public key");

    cmd!(CLIENT_BIN, "config", "new", &rpc.to_string())
        .pipe(cmd!(CLIENT_BIN, "send-asset", "1", &recipient, "10"))
        .run()
        .expect_err("send asset");
}

fn get_balance(config: String) -> usize {
    cmd!(CLIENT_BIN, "get-balance")
        .stdin_bytes(config)
        .read()
        .expect("get asset")
        .parse::<usize>()
        .expect("parse asset amount as usize")
}

#[tokio::test]
async fn new_client_has_some_asset() {
    let (_servers, rpc) = start_network(3).await;

    let config = cmd!(CLIENT_BIN, "config", "new", &rpc.to_string())
        .read()
        .expect("create sender");

    assert!(get_balance(config) > 0);
}

fn transfer(
    sender_config: String,
    sender_sequence: sieve::Sequence,
    receiver_config: String,
    amount: usize,
) {
    let second_client = cmd!(CLIENT_BIN, "config", "get-public-key")
        .stdin_bytes(receiver_config)
        .read()
        .expect("get public key");

    cmd!(
        CLIENT_BIN,
        "send-asset",
        &sender_sequence.to_string(),
        &second_client,
        &amount.to_string(),
    )
    .stdin_bytes(sender_config)
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

async fn wait_for_sequence(config: String, sequence: sieve::Sequence) {
    let timeout = Instant::now() + TIMEOUT;
    while Instant::now() < timeout {
        let last_sequence = get_last_sequence(config.clone());
        if last_sequence == sequence {
            return;
        }

        tokio::time::sleep(TICK).await;
    }

    panic!("timeout expired");
}

#[tokio::test]
async fn transfer_increment_sequence() {
    let (_servers, rpc) = start_network(3).await;

    let sender = cmd!(CLIENT_BIN, "config", "new", &rpc.to_string())
        .read()
        .expect("create sender");

    let receiver = cmd!(CLIENT_BIN, "config", "new", &rpc.to_string())
        .read()
        .expect("create receiver");

    let sequence = get_last_sequence(sender.clone());

    transfer(sender.clone(), sequence + 1, receiver.clone(), 1);

    wait_for_sequence(sender.clone(), sequence + 1).await;
}

#[tokio::test]
async fn can_send_asset() {
    const AMOUNT: usize = 10;

    let (_servers, rpc) = start_network(3).await;

    let sender = cmd!(CLIENT_BIN, "config", "new", &rpc.to_string())
        .read()
        .expect("create sender");

    let receiver = cmd!(CLIENT_BIN, "config", "new", &rpc.to_string())
        .read()
        .expect("create receiver");

    transfer(sender.clone(), 1, receiver.clone(), AMOUNT);

    wait_for_sequence(sender.clone(), 1).await;

    assert_eq!(get_balance(sender) + AMOUNT, get_balance(receiver) - AMOUNT);
}
