mod execution;
mod market;
pub mod provider_agent;
mod startup_config;
mod utils;

use crate::provider_agent::ProviderAgent;
use crate::startup_config::StartupConfig;

use actix::prelude::*;
use log::info;
use structopt::StructOpt;

fn main() {
    dotenv::dotenv().ok();
    env_logger::init();
    info!("Running Provider Agent.");

    let args = StartupConfig::from_args();
    let system = System::new("ProviderAgent");

    ProviderAgent::new(args).unwrap().start();
    system.run().unwrap();
}
