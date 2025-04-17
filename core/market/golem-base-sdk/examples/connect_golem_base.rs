use clap::Parser;
use log::LevelFilter;
use url::Url;

use golem_base_sdk::client::GolemBaseClient;

/// Simple program to connect to a Geth node
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// URL of the Geth node to connect to
    #[arg(short, long, default_value = "http://localhost:8545")]
    url: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logger
    env_logger::Builder::new()
        .filter_level(LevelFilter::Info)
        .init();

    let args = Args::parse();

    // Parse the URL
    let endpoint = Url::parse(&args.url)?;
    log::info!("Connecting to Geth node at: {}", endpoint);

    // Create the client
    let client = GolemBaseClient::new(endpoint);

    // Check connection by getting chain ID
    let chain_id = client.get_chain_id().await?;
    log::info!(
        "Successfully connected to Geth node. Chain ID: {}",
        chain_id
    );

    Ok(())
}
