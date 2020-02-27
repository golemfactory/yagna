mod execution;
mod market;
pub mod provider_agent;
mod startup_config;

use crate::provider_agent::ProviderAgent;
use crate::startup_config::StartupConfig;

use actix::prelude::*;
use structopt::StructOpt;

fn main() {
    dotenv::dotenv().ok();
    env_logger::init();
    log::info!("Running Provider Agent.");

    let args = StartupConfig::from_args();
    let system = System::new("ProviderAgent");

    ProviderAgent::new(args).unwrap().start();
    system.run().unwrap();
}
