use std::env;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::{net::TcpStream, time::sleep};

use duct::cmd;

const SERVER_BIN: &str = env!("CARGO_BIN_EXE_server");

struct Server {
    handle: duct::Handle,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.handle.kill().expect("kill server");
    }
}

async fn start_server(ip: &SocketAddr) -> Server {
    let config = cmd!(SERVER_BIN, "config", "new", ip.to_string())
        .stdout_capture()
        .run()
        .expect("generate config");

    let server = cmd!(SERVER_BIN, "run")
        .stdin_bytes(config.stdout)
        .start()
        .expect("run server");

    while let Err(err) = TcpStream::connect(ip).await {
        if err.kind() != io::ErrorKind::ConnectionRefused {
            panic!("connect server: {}", err)
        }
        sleep(Duration::from_millis(10)).await;
    }

    Server { handle: server }
}

#[tokio::test]
async fn can_run_server() {
    let ip: SocketAddr = (Ipv4Addr::new(127, 0, 0, 1), 3000).into();
    start_server(&ip).await;
}
