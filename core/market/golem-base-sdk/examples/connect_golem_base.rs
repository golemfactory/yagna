use anyhow::Result;
use clap::Parser;
use url::Url;

use golem_base_sdk::client::GolemBaseClient;
use golem_base_sdk::entity::Create;

/// Simple program to connect to a Geth node
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// URL of the Geth node to connect to
    #[arg(short, long, default_value = "http://localhost:8545")]
    url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();

    // Connect to GolemBase
    let endpoint = Url::parse(&args.url)?;
    let client = GolemBaseClient::new(endpoint);

    // Get accounts
    let accounts = client.sync_accounts().await?;
    log::info!("Available accounts: {:?}", accounts);

    // Take the first account
    let account = accounts
        .first()
        .ok_or_else(|| anyhow::anyhow!("No accounts available"))?;
    log::info!("Using account: {:?}", account);

    // Create a test entry
    let test_payload = b"test payload".to_vec();
    let entry = Create::new(test_payload.clone(), 1000);

    // Create entry with the account
    let entry_id = client
        .create_entry(*account, entry)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create entry: {e}"))?;
    log::info!("Entry created with ID: {:?}", entry_id);

    Ok(())
}
