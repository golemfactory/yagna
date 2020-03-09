use actix::prelude::*;
use actix::utils::IntervalFunc;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ya_agent_offer_model::{InfNodeInfo, NodeInfo, OfferDefinition, ServiceInfo};
use ya_utils_actix::{actix_handler::send_message, actix_signal::Subscribe};

use crate::execution::{InitializeExeUnits, TaskRunner, UpdateActivity, ActivityCreated, ActivityDestroyed};
use crate::market::{
    provider_market::{AgreementSigned, OnShutdown, UpdateMarket},
    CreateOffer, ProviderMarket,
};
use crate::payments::Payments;
use crate::startup_config::StartupConfig;

pub struct ProviderAgent {
    market: Addr<ProviderMarket>,
    runner: Addr<TaskRunner>,
    payments: Addr<Payments>,
    node_info: NodeInfo,
    service_info: ServiceInfo,
    exe_unit_path: String,
}

impl ProviderAgent {
    pub async fn new(config: StartupConfig) -> anyhow::Result<ProviderAgent> {
        let market = ProviderMarket::new(config.market_client()?, "AcceptAll").start();
        let runner = TaskRunner::new(config.activity_client()?).start();
        let payments = Payments::new(config.activity_client()?, config.payment_client()?).start();

        let node_info = ProviderAgent::create_node_info();
        let service_info = ProviderAgent::create_service_info();

        let exe_unit_path = format!(
            "{}/example-exeunits.json",
            match config.exe_unit_path.is_none() {
                true => {
                    let global_path_linux = "/usr/lib/yagna/plugins";
                    if cfg!(target_os = "linux") && Path::new(global_path_linux).exists() {
                        global_path_linux.into()
                    } else {
                        "exe-unit".into()
                    }
                }
                false => config.exe_unit_path.unwrap(),
            }
        );
        log::debug!("Exe unit configuration path: {}", exe_unit_path);

        let mut provider = ProviderAgent {
            market,
            runner,
            payments,
            node_info,
            service_info,
            exe_unit_path,
        };
        provider.initialize().await?;

        Ok(provider)
    }

    pub async fn initialize(&mut self) -> anyhow::Result<()> {
        // Forward AgreementSigned event to TaskRunner actor.
        let msg = Subscribe::<AgreementSigned>(self.runner.clone().recipient());
        send_message(self.market.clone(), msg);

        let msg = Subscribe::<AgreementSigned>(self.payments.clone().recipient());
        send_message(self.market.clone(), msg);

        //
        let msg = Subscribe::<ActivityCreated>(self.payments.clone().recipient());
        send_message(self.runner.clone(), msg);

        let msg = Subscribe::<ActivityDestroyed>(self.payments.clone().recipient());
        send_message(self.runner.clone(), msg);

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
        Ok(self.market.clone().send(create_offer_message).await??)
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
