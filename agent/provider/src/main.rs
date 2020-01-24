mod execution;
mod market;
mod node_info;
pub mod provider_agent;
mod utils;

use ya_client::web::WebAuth;
use crate::provider_agent::ProviderAgent;

use actix::prelude::*;
use log::info;
use structopt::StructOpt;


#[derive(StructOpt)]
struct Args {
    auth: String
}


fn main() {
    env_logger::init();
    info!("Running Provider Agent.");

    let args = Args::from_args();
    let system = System::new("ProviderAgent");

    ProviderAgent::new(WebAuth::Bearer(args.auth)).unwrap().start();
    system.run().unwrap();
}
