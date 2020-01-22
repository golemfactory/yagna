use ya_client::{market::ApiClient, web::WebClient, Result};

use crate::market::{ProviderMarketActor, CreateOffer};
use crate::execution::TaskRunner;
use crate::node_info::{CpuInfo, NodeInfo};
//use crate::utils::actix_handler::send_message;

use actix::prelude::*;
use actix::utils::IntervalFunc;
use log::{error};
use std::time::Duration;
use crate::market::provider_market::UpdateMarket;


#[allow(dead_code)]
pub struct ProviderAgent {
    market: Addr<ProviderMarketActor>,
    runner: TaskRunner,     ///TODO: Should be actix actor.
    node_info: NodeInfo,
}

impl Actor for ProviderAgent {
    type Context = Context<Self>;

    fn started(&mut self, context: &mut Context<Self>) {
        IntervalFunc::new(Duration::from_secs(4), Self::schedule_jobs)
            .finish()
            .spawn(context);
    }
}


impl ProviderAgent {
    pub fn new() -> Result<ProviderAgent> {
        let client = ApiClient::new(WebClient::builder())?;
        let market = ProviderMarketActor::new(client, "AcceptAll").start();
        let runner = TaskRunner::new();

        let node_info = ProviderAgent::create_node_info();

        let mut provider = ProviderAgent{ market, runner, node_info };
        provider.initialize();

        Ok(provider)
    }

    pub fn initialize(&mut self) {
        let create_offer_message = CreateOffer::new(ProviderAgent::create_node_info());
        let market = self.market.clone();

        let future = async move {
            if let Err(error) = market.send(create_offer_message).await {
                error!("Error creating initial offer: {}.", error);
            };
        };
        Arbiter::spawn(future);

        //TODO: Use
        //send_message(self.market.clone(), create_offer_message);
    }

    fn schedule_jobs(&mut self, _ctx: &mut Context<Self>) {
        let market = self.market.clone();

        let future = async move {
            if let Err(error) = market.send(UpdateMarket).await {
                error!("Error while sending UpdateMarket message: {}.", error);
            };
        };
        Arbiter::spawn(future);

        //TODO: Use
        //send_message(self.market.clone(), UpdateMarket);
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
