use std::{
    env, io,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::PathBuf,
    time::Duration,
};
use tokio::{net::TcpStream, time::sleep};

use duct::cmd;

fn cargo_bin(name: &str) -> io::Result<PathBuf> {
    env::current_exe().map(|mut path| {
        path.pop();
        if path.ends_with("deps") {
            path.pop();
        }
        path.push(name);
        path
    })
}

fn server_bin() -> PathBuf {
    cargo_bin("server").expect("server binary not found")
}

async fn start_server(ip: &SocketAddr) -> duct::Handle {
    let config = cmd!(server_bin(), "config", "new", ip.to_string())
        .stdout_capture()
        .run()
        .expect("generate config");

    let server = cmd!(server_bin(), "run")
        .stdin_bytes(config.stdout)
        .start()
        .expect("run server");

    while let Err(err) = TcpStream::connect(ip).await {
        if err.kind() != io::ErrorKind::ConnectionRefused {
            panic!("connect server: {}", err)
        }
        sleep(Duration::from_millis(10)).await;
    }

    server
}

#[tokio::test]
async fn can_run_server() {
    let ip = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 3000));
    let server = start_server(&ip).await;

    server.kill().expect("kill server");
}
