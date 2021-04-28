use std::io::{self, Read};
use std::net::SocketAddr;
use std::process;

use snafu::{ResultExt, Snafu};
use structopt::StructOpt;
use tonic::transport::Server;

mod config;
mod rpc;

#[derive(Debug, StructOpt)]
enum Commands {
    Config(CommandsConfig),
    Run,
}

#[derive(Debug, StructOpt)]
enum CommandsConfig {
    New { address: SocketAddr },
}

#[derive(Debug, Snafu)]
enum Error {
    ConfigEncode { source: toml::ser::Error },
    RunReadConfig { source: io::Error },
    RunDecodeConfig { source: toml::de::Error },
    RunServer { source: tonic::transport::Error },
}

fn config(cmd: CommandsConfig) -> Result<(), Error> {
    match cmd {
        CommandsConfig::New { address } => {
            let config = config::Config { address };
            let encoded = toml::to_string_pretty(&config).context(ConfigEncode)?;

            println!("{}", encoded);

            Ok(())
        }
    }
}

async fn run() -> Result<(), Error> {
    let mut buffer = String::new();
    io::stdin()
        .read_to_string(&mut buffer)
        .context(RunReadConfig)?;
    let config: config::Config = toml::from_str(&buffer).context(RunDecodeConfig)?;

    let service = rpc::Service::default();

    Server::builder()
        .add_service(rpc::At2Server::new(service))
        .serve(config.address)
        .await
        .context(RunServer)?;

    Ok(())
}

#[tokio::main]
async fn main() {
    let ret = match Commands::from_args() {
        Commands::Config(cmd) => config(cmd),
        Commands::Run => run().await,
    };

    if let Err(err) = ret {
        eprintln!("error running cmd: {}", err);
        process::exit(1);
    }
}
