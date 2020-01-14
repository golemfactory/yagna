
use ya_client::{
    market::{ApiClient},
    web::WebClient,
    Result,
};

use crate::market::ProviderMarket;
use crate::node_info::{NodeInfo, CpuInfo};

use std::{thread, time};
use log::{error};



pub struct ProviderAgent {
    market: ProviderMarket,
    node_info: NodeInfo,
}


impl ProviderAgent {

    pub fn new() -> Result< ProviderAgent > {
        let client = ApiClient::new(WebClient::builder())?;
        let market = ProviderMarket::new(client, "AcceptAll");

        let node_info = ProviderAgent::create_node_info();

        Ok(ProviderAgent{market, node_info})
    }

    pub async fn run(&mut self) {

        if let Err(error) = self.market.create_offers(&self.node_info).await {
            error!("Error while starting market: {}", error);
            return ();
        }

        //TODO: We should replace this loop with scheduler in future.
        loop {
            if let Err(error) = self.market.run_step().await {
                error!("Market error: {}", error)
            }

            thread::sleep(time::Duration::from_secs(3));
        }

        // We never get here, but we should cleanup market in final version.
        if let Err(error) = self.market.onshutdown().await {
            error!("Error while market shutdown: {}", error);
        }
    }

    fn create_node_info() -> NodeInfo {
        let cpu = CpuInfo{ architecture: "wasm32".to_string(), cores: 1, threads: 1 };
         NodeInfo{ cpu, id: "Provider Node".to_string() }
    }

}



