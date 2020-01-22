use ya_client::{market::ApiClient, web::WebClient, Result};
use ya_client::activity::{provider::ProviderApiClient, ACTIVITY_API};

use crate::execution::TaskRunnerActor;
use crate::market::{CreateOffer, ProviderMarketActor};
use crate::node_info::{CpuInfo, NodeInfo};
use crate::utils::actix_handler::send_message;
use crate::utils::actix_signal::Subscribe;

use crate::market::provider_market::{UpdateMarket, AgreementSigned};
use actix::prelude::*;
use actix::utils::IntervalFunc;
use std::time::Duration;
use std::sync::Arc;


#[allow(dead_code)]
pub struct ProviderAgent {
    market: Addr<ProviderMarketActor>,
    ///TODO: Should be actix actor.
    runner: Addr<TaskRunnerActor>,
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

        let client = ProviderApiClient::new(WebClient::builder().api_root(ACTIVITY_API).build().map(Arc::new)?);
        let runner = TaskRunnerActor::new(client).start();

        let node_info = ProviderAgent::create_node_info();

        let mut provider = ProviderAgent {
            market,
            runner,
            node_info,
        };
        provider.initialize();

        Ok(provider)
    }

    pub fn initialize(&mut self) {
        // Forward AgreementSigned event to TaskRunner actor.
        let msg = Subscribe::<AgreementSigned>(self.runner.clone().recipient());
        send_message(self.market.clone(), msg);

        // Create simple offer on market.
        let create_offer_message = CreateOffer::new(ProviderAgent::create_node_info());
        send_message(self.market.clone(), create_offer_message);
    }

    fn schedule_jobs(&mut self, _ctx: &mut Context<Self>) {
        send_message(self.market.clone(), UpdateMarket);
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
