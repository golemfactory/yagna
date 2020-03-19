use structopt::{clap, StructOpt};

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

    let app_name = clap::crate_name!();
    log::info!("Starting {}...", app_name);

    let args = StartupConfig::from_args();
    ProviderAgent::new(args).await?.wait_for_ctrl_c().await
}
