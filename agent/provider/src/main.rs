use actix::prelude::*;
use structopt::StructOpt;

mod execution;
mod market;
mod provider_agent;
mod startup_config;

use provider_agent::ProviderAgent;
use startup_config::StartupConfig;

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();
    log::info!("Running Provider Agent.");

    let args = StartupConfig::from_args();

    ProviderAgent::new(args).await?.start();
    tokio::signal::ctrl_c().await?;
    println!();
    log::info!("SIGINT received, exiting");
    Ok(())
}
