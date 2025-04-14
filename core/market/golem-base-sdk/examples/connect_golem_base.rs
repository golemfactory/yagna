use clap::Parser;
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
    let args = Args::parse();

    // Parse the URL
    let endpoint = Url::parse(&args.url)?;
    println!("Connecting to Geth node at: {}", endpoint);

    // Create the client
    let client = GolemBaseClient::new(endpoint);
    println!("Successfully connected to Geth node");

    Ok(())
}
