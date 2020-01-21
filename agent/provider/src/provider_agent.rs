use ya_client::{market::ApiClient, web::WebClient, Result};

use crate::market::{ProviderMarketActor, ProviderMarket};
use crate::execution::TaskRunner;
use crate::node_info::{CpuInfo, NodeInfo};

use log::error;
use std::{thread, time};
use actix::prelude::*;

pub struct ProviderAgent {
    market: Addr<ProviderMarketActor>,
    runner: TaskRunner,     ///TODO: Should be actix actor.
    node_info: NodeInfo,
}

impl Actor for ProviderAgent {
    type Context = Context<Self>;
}


impl ProviderAgent {
    pub fn new() -> Result<ProviderAgent> {
        let client = ApiClient::new(WebClient::builder())?;
        let market = ProviderMarketActor::new(client, "AcceptAll").start();
        let runner = TaskRunner::new();

        let node_info = ProviderAgent::create_node_info();

        Ok(ProviderAgent{ market, runner, node_info })
    }

    pub async fn run(&mut self) {
//        if let Err(error) = self.market.create_offer(&self.node_info).await {
//            error!("Error while starting market: {}", error);
//            return ();
//        }
//
//        //TODO: We should replace this loop with scheduler in future.
//        loop {
//            if let Err(error) = self.market.run_step().await {
//                error!("Market error: {}", error)
//            }
//
//            thread::sleep(time::Duration::from_secs(3));
//        }

        // We never get here, but we should cleanup market in final version.
        //        if let Err(error) = self.market.onshutdown().await {
        //            error!("Error while market shutdown: {}", error);
        //        }
    }

    fn create_node_info() -> NodeInfo {
        let cpu = CpuInfo {
            architecture: "wasm32".to_string(),
            cores: 1,
            threads: 1,
        };
        NodeInfo {
            cpu,
            id: "Provider Node".to_string(),
        }
    }
}
