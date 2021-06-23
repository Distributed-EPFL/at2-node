use std::io::{stdin, stdout};

use drop::crypto::sign;

use at2_node::{proto, Transaction};

use snafu::{ResultExt, Snafu};
use structopt::StructOpt;
use url::Url;

mod config;

#[derive(Debug, StructOpt)]
enum Commands {
    Config(CommandsConfig),
    SendAsset {
        recipient: sign::PublicKey,
        amount: u64,
    },
    GetBalance,
    GetLastSequence,
}

#[derive(Debug, StructOpt)]
enum CommandsConfig {
    New { rpc_address: Url },
    GetPublicKey,
}

#[derive(Debug, Snafu)]
enum CommandError {
    #[snafu(display("read config: {}", source))]
    ReadConfig { source: config::Error },
    #[snafu(display("serialize: {}", source))]
    Serialize { source: bincode::Error },
    #[snafu(display("transport: {}", source))]
    Transport { source: tonic::transport::Error },
    #[snafu(display("send asset: {}", source))]
    Rpc { source: tonic::Status },
}

#[derive(Debug, Snafu)]
enum CommandsError {
    #[snafu(display("config: {}", source))]
    Config { source: config::Error },
    #[snafu(display("send asset: {}", source))]
    SendAsset { source: CommandError },
    #[snafu(display("get asset: {}", source))]
    GetBalance { source: CommandError },
    #[snafu(display("get last sequence: {}", source))]
    GetLastSequence { source: CommandError },
}

fn config(cmd: CommandsConfig) -> Result<(), config::Error> {
    match cmd {
        CommandsConfig::New { rpc_address } => config::Config {
            rpc_address,
            private_key: sign::KeyPair::random().private(),
        }
        .to_writer(stdout()),
        CommandsConfig::GetPublicKey => {
            let config = config::from_reader(stdin())?;

            println!("{}", sign::KeyPair::from(config.private_key).public());

            Ok(())
        }
    }
}

async fn send_asset(recipient: sign::PublicKey, amount: u64) -> Result<(), CommandError> {
    let config = config::from_reader(stdin()).context(ReadConfig)?;

    let sign_keypair = sign::KeyPair::from(config.private_key);
    let message = Transaction { recipient, amount };
    let signature = sign_keypair.sign(&message).expect("sign failed");

    let mut client = proto::At2Client::connect(config.rpc_address.to_string())
        .await
        .context(Transport)?;

    let request = tonic::Request::new(proto::SendAssetRequest {
        sender: bincode::serialize(&sign_keypair.public()).context(Serialize)?,
        sequence: 0, // TODO store sequence somewhere
        receiver: bincode::serialize(&recipient).context(Serialize)?,
        amount,
        signature: bincode::serialize(&signature).context(Serialize)?,
    });

    client.send_asset(request).await.context(Rpc)?;

    Ok(())
}

async fn get_balance() -> Result<(), CommandError> {
    let config = config::from_reader(stdin()).context(ReadConfig)?;

    let reply = proto::At2Client::connect(config.rpc_address.to_string())
        .await
        .context(Transport)?
        .get_balance(tonic::Request::new(proto::GetBalanceRequest {
            sender: bincode::serialize(&sign::KeyPair::from(config.private_key).public())
                .context(Serialize)?,
        }))
        .await
        .context(Rpc)?;

    println!("{}", reply.get_ref().amount);

    Ok(())
}

async fn get_last_sequence() -> Result<(), CommandError> {
    let config = config::from_reader(stdin()).context(ReadConfig)?;

    let request = tonic::Request::new(proto::GetLastSequenceRequest {
        sender: bincode::serialize(&sign::KeyPair::from(config.private_key).public())
            .context(Serialize)?,
    });

    let reply = proto::At2Client::connect(config.rpc_address.to_string())
        .await
        .context(Transport)?
        .get_last_sequence(request)
        .await
        .context(Rpc)?;

    println!("{}", reply.get_ref().sequence);

    Ok(())
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let ret = match Commands::from_args() {
        Commands::Config(cmd) => config(cmd).context(Config),
        Commands::SendAsset { recipient, amount } => {
            send_asset(recipient, amount).await.context(SendAsset)
        }
        Commands::GetBalance => get_balance().await.context(GetBalance),
        Commands::GetLastSequence => get_last_sequence().await.context(GetLastSequence),
    };

    if let Err(err) = ret {
        eprintln!("error running cmd: {}", err);
        std::process::exit(1);
    }
}
