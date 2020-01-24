use ya_client::activity::{provider::ProviderApiClient, ACTIVITY_API};
use ya_client::{market::ApiClient, web::WebClient, web::WebAuth, Result};

use crate::execution::{InitializeExeUnits, TaskRunnerActor, UpdateActivity};
use crate::market::{CreateOffer, ProviderMarketActor};
use crate::node_info::{CpuInfo, NodeInfo};
use crate::utils::actix_handler::send_message;
use crate::utils::actix_signal::Subscribe;

use crate::market::provider_market::{AgreementSigned, OnShutdown, UpdateMarket};
use actix::prelude::*;
use actix::utils::IntervalFunc;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub struct ProviderAgent {
    market: Addr<ProviderMarketActor>,
    runner: Addr<TaskRunnerActor>,
    node_info: NodeInfo,
}

impl ProviderAgent {
    pub fn new(auth: WebAuth) -> Result<ProviderAgent> {
        let client = ApiClient::new(WebClient::builder().host_port("10.30.8.179:5001").auth(auth))?;
        let market = ProviderMarketActor::new(client, "AcceptAll").start();

        let client = ProviderApiClient::new(
            WebClient::builder()
                .api_root(ACTIVITY_API)
                .build()
                .map(Arc::new)?,
        );
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

        // Load ExeUnits descriptors from file.
        // TODO: Hardcoded exeunits file. How should we handle this in future?
        let exeunits_file = PathBuf::from("exe-unit/example-exeunits.json");
        let msg = InitializeExeUnits {
            file: exeunits_file,
        };
        send_message(self.runner.clone(), msg);

        // Create simple offer on market.
        let create_offer_message = CreateOffer::new(self.node_info.clone());
        send_message(self.market.clone(), create_offer_message);
    }

    fn schedule_jobs(&mut self, _ctx: &mut Context<Self>) {
        send_message(self.market.clone(), UpdateMarket);
        send_message(self.runner.clone(), UpdateActivity);
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

    pub fn spawn_shutdown_handler(&mut self, context: &mut Context<ProviderAgent>) {
        let market = self.market.clone();
        let _ = context.spawn(
            async move {
                let _ = tokio::signal::ctrl_c().await;
                log::info!("Shutting down system.");

                let _ = market.send(OnShutdown {}).await;
                System::current().stop();
            }
            .into_actor(self),
        );
    }
}

impl Actor for ProviderAgent {
    type Context = Context<Self>;

    fn started(&mut self, context: &mut Context<Self>) {
        IntervalFunc::new(Duration::from_secs(4), Self::schedule_jobs)
            .finish()
            .spawn(context);

        self.spawn_shutdown_handler(context);
    }
}

