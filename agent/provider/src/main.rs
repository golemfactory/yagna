pub mod provider_agent;
mod market;

use futures::executor::block_on;
use crate::provider_agent::ProviderAgent;


fn main() {
    println!("Mock Provider Agent!");
    let agent = ProviderAgent::new().unwrap();
    block_on(agent.run());
}
