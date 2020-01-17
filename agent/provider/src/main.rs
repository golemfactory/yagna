pub mod provider_agent;
mod node_info;
mod market;

use crate::provider_agent::ProviderAgent;
use log::{info};


async fn run_main() {
    let mut agent = ProviderAgent::new().unwrap();
    agent.run().await;
}

fn main() {
    env_logger::init();
    info!("Running Provider Agent.");

    actix_rt::System::new("test")
        .block_on(run_main());
}
