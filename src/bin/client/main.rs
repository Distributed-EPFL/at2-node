use std::io::{stdin, stdout};

use at2_node::{
    client::{self, Client},
    TransactionState,
};
use drop::crypto::sign;
use hex::FromHex;
use http::Uri;
use snafu::{ResultExt, Snafu};
use structopt::StructOpt;

mod config;

fn hex_decode<T: FromHex>(src: &str) -> Result<T, T::Error> {
    T::from_hex(src)
}

#[derive(Debug, StructOpt)]
enum Commands {
    Config(CommandsConfig),
    SendAsset {
        sequence: sieve::Sequence,
        #[structopt(parse(try_from_str = hex_decode))]
        recipient: sign::PublicKey,
        amount: u64,
    },
    GetBalance,
    GetLastSequence,
    GetLatestTransactions,
}

#[derive(Debug, StructOpt)]
enum CommandsConfig {
    New { rpc_address: Uri },
    GetPublicKey,
}

#[derive(Debug, Snafu)]
enum CommandError {
    #[snafu(display("read config: {}", source))]
    ReadConfig { source: config::Error },
    #[snafu(display("serialize: {}", source))]
    Serialize { source: bincode::Error },
    #[snafu(display("client: {}", source))]
    ClientError { source: client::Error },
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
    #[snafu(display("get latest transactions: {}", source))]
    GetLatestTransactions { source: CommandError },
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

async fn send_asset(
    sequence: sieve::Sequence,
    recipient: sign::PublicKey,
    amount: u64,
) -> Result<(), CommandError> {
    let config = config::from_reader(stdin()).context(ReadConfig)?;

    Client::new(config.rpc_address)
        .send_asset(
            &sign::KeyPair::from(config.private_key),
            sequence,
            recipient,
            amount,
        )
        .await
        .context(ClientError)?;

    Ok(())
}

async fn get_balance() -> Result<(), CommandError> {
    let config = config::from_reader(stdin()).context(ReadConfig)?;

    let amount = Client::new(config.rpc_address)
        .get_balance(&sign::KeyPair::from(config.private_key).public())
        .await
        .context(ClientError)?;

    println!("{}", amount);

    Ok(())
}

async fn get_last_sequence() -> Result<(), CommandError> {
    let config = config::from_reader(stdin()).context(ReadConfig)?;

    let sequence = Client::new(config.rpc_address)
        .get_last_sequence(&sign::KeyPair::from(config.private_key).public())
        .await
        .context(ClientError)?;

    println!("{}", sequence);

    Ok(())
}

async fn get_latest_transactions() -> Result<(), CommandError> {
    let config = config::from_reader(stdin()).context(ReadConfig)?;

    Client::new(config.rpc_address)
        .get_latest_transactions()
        .await
        .context(ClientError)?
        .iter()
        .for_each(|tx| {
            println!(
                "{}: {} send {}¤ to {} ({})",
                tx.timestamp,
                tx.sender,
                tx.amount,
                tx.recipient,
                match tx.state {
                    TransactionState::Pending => "pending",
                    TransactionState::Success => "success",
                    TransactionState::Failure => "failure",
                },
            )
        });

    Ok(())
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let ret = match Commands::from_args() {
        Commands::Config(cmd) => config(cmd).context(Config),
        Commands::SendAsset {
            sequence,
            recipient,
            amount,
        } => send_asset(sequence, recipient, amount)
            .await
            .context(SendAsset),
        Commands::GetBalance => get_balance().await.context(GetBalance),
        Commands::GetLastSequence => get_last_sequence().await.context(GetLastSequence),
        Commands::GetLatestTransactions => get_latest_transactions()
            .await
            .context(GetLatestTransactions),
    };

    if let Err(err) = ret {
        eprintln!("error running cmd: {}", err);
        std::process::exit(1);
    }
}
