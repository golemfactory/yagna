mod execution;
mod market;
mod node_info;
mod startup_config;
pub mod provider_agent;
mod utils;

use crate::provider_agent::ProviderAgent;
use crate::startup_config::StartupConfig;

use actix::prelude::*;
use log::info;
use structopt::StructOpt;



fn main() {
    env_logger::init();
    info!("Running Provider Agent.");

    let args = StartupConfig::from_args();
    let system = System::new("ProviderAgent");

    ProviderAgent::new(args).unwrap().start();
    system.run().unwrap();
}
