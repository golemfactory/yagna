use ya_client::activity::provider::ProviderApiClient;
use ya_client::{market::ApiClient, Result};

use crate::execution::{InitializeExeUnits, TaskRunnerActor, UpdateActivity};
use crate::market::{CreateOffer, ProviderMarketActor};
use crate::startup_config::StartupConfig;
use crate::utils::actix_handler::send_message;
use crate::utils::actix_signal::Subscribe;

use crate::market::provider_market::{AgreementSigned, OnShutdown, UpdateMarket};
use actix::prelude::*;
use actix::utils::IntervalFunc;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ya_agent_offer_model::{InfNodeInfo, NodeInfo, OfferDefinition, ServiceInfo};

pub struct ProviderAgent {
    market: Addr<ProviderMarketActor>,
    runner: Addr<TaskRunnerActor>,
    node_info: NodeInfo,
    service_info: ServiceInfo,
    exe_unit_path: String,
}

impl ProviderAgent {
    pub fn new(config: StartupConfig) -> Result<ProviderAgent> {
        let webclient = config.market_client();

        let client = ApiClient::new(webclient)?;
        let market = ProviderMarketActor::new(client, "AcceptAll").start();

        let client = ProviderApiClient::new(Arc::new(config.activity_client().build()?));
        let runner = TaskRunnerActor::new(client).start();

        let node_info = ProviderAgent::create_node_info();
        let service_info = ProviderAgent::create_service_info();

        let exe_unit_path = format!(
            "{}/example-exeunits.json",
            match config.exe_unit_path.is_empty() {
                true => "exe-unit".into(),
                false => config.exe_unit_path,
            }
        );

        let mut provider = ProviderAgent {
            market,
            runner,
            node_info,
            service_info,
            exe_unit_path,
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
        let exeunits_file = PathBuf::from(
            self.exe_unit_path.clone(), /*"exe-unit/example-exeunits.json"*/
        );
        let msg = InitializeExeUnits {
            file: exeunits_file,
        };
        send_message(self.runner.clone(), msg);

        // Create simple offer on market.
        let create_offer_message = CreateOffer::new(OfferDefinition {
            node_info: self.node_info.clone(),
            service: self.service_info.clone(),
            com_info: Default::default(),
        });
        send_message(self.market.clone(), create_offer_message);
    }

    fn schedule_jobs(&mut self, _ctx: &mut Context<Self>) {
        send_message(self.market.clone(), UpdateMarket);
        send_message(self.runner.clone(), UpdateActivity);
    }

    fn create_node_info() -> NodeInfo {
        // TODO: Get node name from intentity API.
        NodeInfo::with_name("")
    }

    fn create_service_info() -> ServiceInfo {
        let inf = InfNodeInfo::new().with_mem(1.0).with_storage(10.0);
        let wasi_version = "0.0.0".into();
        ServiceInfo::Wasm { inf, wasi_version }
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
