use alloy::primitives::{keccak256, Address};
use anyhow::Result;
use clap::Parser;
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;
use ya_client_model::NodeId;

use bigdecimal::BigDecimal;
use golem_base_sdk::client::GolemBaseClient;
use golem_base_sdk::entity::Create;

/// Simple program to connect to a Geth node
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// URL of the Geth node to connect to
    #[arg(short, long, default_value = "http://localhost:8545")]
    url: String,

    /// NodeId of the wallet to use (optional)
    #[arg(short, long)]
    wallet: Option<NodeId>,

    /// Password for the wallet (optional, defaults to "test123")
    #[arg(short, long, default_value = "test123")]
    password: String,

    /// Entry to store in Golem Base (defaults to "test payload")
    #[arg(short, long, default_value = "test payload")]
    entry: String,

    /// Skip funding the account
    #[arg(short, long, default_value = "false")]
    skip_fund: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();

    // Connect to GolemBase
    let endpoint = Url::parse(&args.url)?;
    let client = GolemBaseClient::new(endpoint).await?;

    // Get accounts
    let accounts = client
        .account_sync()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to sync accounts: {e}"))?;
    log::info!("Available accounts: {:?}", accounts);

    // Log balances for all accounts
    for &addr in &accounts {
        let balance = client.get_balance(addr).await?;
        log::info!("Account {} balance: {} ETH", addr, balance);
    }

    // Select account based on command line argument or generate new one
    let account = if let Some(wallet) = args.wallet {
        let wallet_address = Address::from(&wallet.into_array());
        if !accounts.contains(&wallet_address) {
            return Err(anyhow::anyhow!(
                "Specified wallet {wallet} not found in available accounts"
            ));
        }
        client.account_load(wallet_address, &args.password).await?
    } else {
        // Generate new account if none specified
        log::info!("No address provided. Generating new account..");
        client.account_generate(&args.password)?
    };
    log::info!("Using account: {account:?}");

    if !args.skip_fund {
        // Fund the account with 1 ETH
        let fund_tx = client
            .fund(account, BigDecimal::from(1))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fund account: {e}"))?;
        log::info!("Account funded with transaction: {:?}", fund_tx);

        // Check account balance
        let account_obj = client.account_get(account)?;
        let balance = account_obj.get_balance().await?;
        log::info!("Account balance: {} ETH", balance);
    }

    // Create a test entry
    let test_payload = args.entry.as_bytes().to_vec();
    let hash = format!("0x{:x}", keccak256(&test_payload));
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| anyhow::anyhow!("Failed to get current timestamp: {e}"))?
        .as_secs();

    log::info!("Offer hash: {hash}");
    log::info!("Timestamp: {timestamp}");
    let entry = Create::new(test_payload.clone(), 1000)
        .annotate_string("golem_marketplace_type", "Offer")
        .annotate_string("golem_marketplace_id", hash)
        .annotate_number("golem_marketplace_timestamp", timestamp as u64);

    // Create entry with the account
    let entry_id = client
        .create_entry(account, entry)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create entry: {e}"))?;
    log::info!("Entry created with ID: {:?}", entry_id);

    // Get the entry
    let entry = client
        .cat(entry_id.clone())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get entry {entry_id}: {e}"))?;
    log::info!("Entry: {entry}");

    // Query for Offers
    let query = format!("golem_marketplace_type = \"Offer\"");
    log::info!("Querying entities with: {}", query);

    let offers = client
        .query_entities(&query)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to query entities: {e}"))?;
    for offer in offers {
        log::info!("Offer: {:?}", offer);
    }

    Ok(())
}
