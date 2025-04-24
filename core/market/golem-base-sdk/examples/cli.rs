use alloy::primitives::{Address, U256};
use anyhow::Result;
use clap::{Parser, Subcommand};
use url::Url;
use ya_client_model::NodeId;

use golem_base_sdk::client::GolemBaseClient;

/// Program to fund and transfer funds between accounts on Golem Base
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// URL of the Geth node to connect to
    #[arg(short, long, default_value = "http://localhost:8545")]
    url: String,

    /// Command to execute
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List all accounts and their balances
    List,
    /// Fund an account with ETH
    Fund {
        /// NodeId of the wallet to fund
        #[arg(short, long)]
        wallet: NodeId,

        /// Amount in ETH to fund
        #[arg(short, long, default_value = "1.0")]
        amount: f64,
    },
    /// Transfer ETH to another account
    Transfer {
        /// NodeId of the source wallet
        #[arg(short, long)]
        from: NodeId,

        /// NodeId of the destination wallet
        #[arg(short, long)]
        to: NodeId,

        /// Amount in ETH to transfer
        #[arg(short, long)]
        amount: f64,

        /// Password for the source wallet
        #[arg(short, long, default_value = "test123")]
        password: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();

    // Connect to GolemBase
    let endpoint = Url::parse(&args.url)?;
    let client = GolemBaseClient::new(endpoint).await?;

    // Sync accounts first
    let accounts = client.account_sync().await?;

    match args.command {
        Command::List => {
            log::info!("Available accounts:");
            for &addr in &accounts {
                let balance = client.get_balance(addr).await?;
                let balance_eth = balance / U256::from(1_000_000_000_000_000_000u128);
                log::info!("  {}: {} ETH", addr, balance_eth.to_string());
            }
        }
        Command::Fund { wallet, amount } => {
            let account = Address::from(&wallet.into_array());
            let account_obj = client.account_load(account, "test123").await?;
            log::info!("Using account: {account_obj:?}");

            // Convert ETH amount to wei
            let amount_wei = U256::from((amount * 1_000_000_000_000_000_000.0) as u128);
            let fund_tx = client.fund(account, amount_wei).await?;
            log::info!("Account funded with transaction: {:?}", fund_tx);
        }
        Command::Transfer {
            from,
            to,
            amount,
            password,
        } => {
            let from_address = Address::from(&from.into_array());
            let to_address = Address::from(&to.into_array());

            // Convert ETH amount to wei
            let amount_wei = U256::from((amount * 1_000_000_000_000_000_000.0) as u128);

            // Load source account
            let account = client.account_load(from_address, &password).await?;
            log::info!("Using account: {account:?}");

            // Transfer funds
            let transfer_tx = client
                .transfer(from_address, to_address, amount_wei)
                .await?;
            log::info!("Transfer transaction: {:?}", transfer_tx);
        }
    }

    Ok(())
}
