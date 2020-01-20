mod market;
mod node_info;
pub mod provider_agent;
mod utils;

use crate::provider_agent::ProviderAgent;

use log::info;
use actix::prelude::*;

async fn run_main() {
    let mut agent = ProviderAgent::new().unwrap();
    agent.run().await;
}

fn main() {
    env_logger::init();
    info!("Running Provider Agent.");

    let system = System::new("ProviderAgent");

    ProviderAgent::new().unwrap().start();

    system.run();

    //actix_rt::System::new("test").block_on(run_main());
}
