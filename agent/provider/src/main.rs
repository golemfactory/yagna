mod market;
mod execution;
mod node_info;
pub mod provider_agent;
mod utils;

use crate::provider_agent::ProviderAgent;

use log::info;
use actix::prelude::*;


fn main() {
    env_logger::init();
    info!("Running Provider Agent.");

    let system = System::new("ProviderAgent");

    ProviderAgent::new().unwrap().start();
    system.run();
}
