use alloy::primitives::Address;
use anyhow::Result;
use bigdecimal::BigDecimal;
use clap::{Parser, Subcommand};
use golem_base_sdk::client::GolemBaseClient;
use url::Url;
use ya_client_model::NodeId;

/// Program to fund and transfer funds between accounts on Golem Base
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// URL of the GolemBase node
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
        amount: BigDecimal,
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
        amount: BigDecimal,

        /// Password for the source wallet
        #[arg(short, long, default_value = "test123")]
        password: String,
    },
    /// Get entity by ID
    GetEntity {
        /// Entity ID to get
        id: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();
    let endpoint = Url::parse(&args.url)?;
    let client = GolemBaseClient::new(endpoint)?;

    // Sync accounts first
    let accounts = client.account_sync().await?;

    match args.command {
        Command::List => {
            log::info!("Available accounts:");
            for &addr in &accounts {
                let balance = client.get_balance(addr).await?;
                log::info!("  {}: {} ETH", addr, balance);
            }
        }
        Command::Fund { wallet, amount } => {
            let account = Address::from(&wallet.into_array());
            let account_obj = client.account_load(account, "test123").await?;
            log::info!("Using account: {account_obj:?}");

            let fund_tx = client.fund(account, amount).await?;
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

            // Load source account
            let account = client.account_load(from_address, &password).await?;
            log::info!("Using account: {account:?}");

            // Transfer funds
            let transfer_tx = client.transfer(from_address, to_address, amount).await?;
            log::info!("Transfer transaction: {:?}", transfer_tx);
        }
        Command::GetEntity { id } => {
            let entry = client.cat(id).await?;
            println!("Entry: {}", entry);
        }
    }

    Ok(())
}
