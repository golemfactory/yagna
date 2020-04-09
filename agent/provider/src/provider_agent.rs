use actix::prelude::*;
use actix::utils::IntervalFunc;
use anyhow::anyhow;
use std::path::PathBuf;
use std::time::Duration;

use ya_agent_offer_model::{InfNodeInfo, NodeInfo, OfferDefinition, ServiceInfo};
use ya_utils_actix::{actix_handler::send_message, actix_signal::Subscribe};

use crate::execution::{
    ActivityCreated, ActivityDestroyed, ExeUnitsRegistry, InitializeExeUnits, TaskRunner,
    UpdateActivity,
};
use crate::market::{
    provider_market::{AgreementApproved, OnShutdown, UpdateMarket},
    CreateOffer, Presets, ProviderMarket,
};
use crate::payments::{LinearPricingOffer, Payments};
use crate::startup_config::RunConfig;

pub struct ProviderAgent {
    market: Addr<ProviderMarket>,
    runner: Addr<TaskRunner>,
    payments: Addr<Payments>,
    node_info: NodeInfo,
    service_info: ServiceInfo,
    exe_unit_path: PathBuf,
}

impl ProviderAgent {
    pub async fn new(config: RunConfig) -> anyhow::Result<ProviderAgent> {
        let market = ProviderMarket::new(config.market_client()?, "AcceptAll").start();
        let runner = TaskRunner::new(config.activity_client()?)?.start();
        let payments = Payments::new(
            config.activity_client()?,
            config.payment_client()?,
            &config.credit_address,
        )
        .start();

        let node_info = ProviderAgent::create_node_info();
        let service_info = ProviderAgent::create_service_info();

        let mut provider = ProviderAgent {
            market,
            runner,
            payments,
            node_info,
            service_info,
            exe_unit_path: config.exe_unit_path,
        };
        provider.initialize(config.presets).await?;

        Ok(provider)
    }

    pub async fn initialize(&mut self, presets: Vec<String>) -> anyhow::Result<()> {
        // Forward AgreementApproved event to TaskRunner actor.
        let msg = Subscribe::<AgreementApproved>(self.runner.clone().recipient());
        self.market.send(msg).await??;

        let msg = Subscribe::<AgreementApproved>(self.payments.clone().recipient());
        self.market.send(msg).await??;

        //
        let msg = Subscribe::<ActivityCreated>(self.payments.clone().recipient());
        self.runner.send(msg).await??;

        let msg = Subscribe::<ActivityDestroyed>(self.payments.clone().recipient());
        self.runner.send(msg).await??;

        // Load ExeUnits descriptors from file.
        let msg = InitializeExeUnits {
            file: PathBuf::from(&self.exe_unit_path),
        };
        self.runner.send(msg).await??;

        Ok(self.create_offers(presets).await?)
    }

    async fn create_offers(&mut self, presets_names: Vec<String>) -> anyhow::Result<()> {
        if presets_names.is_empty() {
            return Err(anyhow!("No Presets were selected. Can't create offers."));
        }

        // TODO: Hardcoded presets file path.
        let presets = Presets::new()
            .load_from_file(&PathBuf::from("presets.json"))?
            .list_matching(presets_names);

        for preset in presets.into_iter() {
            let com_info = match preset.pricing_model.as_str() {
                "linear" => LinearPricingOffer::from_preset(&preset)?
                    .interval(6.0)
                    .build(),
                _ => {
                    return Err(anyhow!(
                        "Unsupported pricing model: {}.",
                        preset.pricing_model
                    ))
                }
            };

            // Create simple offer on market.
            let create_offer_message = CreateOffer {
                preset,
                offer_definition: OfferDefinition {
                    node_info: self.node_info.clone(),
                    service: self.service_info.clone(),
                    com_info,
                },
            };
            self.market.send(create_offer_message).await??
        }
        Ok(())
    }

    fn schedule_jobs(&mut self, _ctx: &mut Context<Self>) {
        send_message(self.runner.clone(), UpdateActivity);
        send_message(self.market.clone(), UpdateMarket);
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

    pub async fn wait_for_ctrl_c(self) -> anyhow::Result<()> {
        let market = self.market.clone();

        self.start();

        let _ = tokio::signal::ctrl_c().await;
        println!();
        log::info!(
            "SIGINT received, Shutting down {}...",
            structopt::clap::crate_name!()
        );

        market.send(OnShutdown {}).await?
    }

    pub fn list_exeunits(exe_unit_path: PathBuf) -> anyhow::Result<()> {
        let mut registry = ExeUnitsRegistry::new();
        registry.register_exeunits_from_file(&exe_unit_path)?;

        println!("Available ExeUnits:");

        let exeunits = registry.list_exeunits();
        for exeunit in exeunits.iter() {
            println!("{}", exeunit);
        }
        Ok(())
    }

    pub fn list_presets(presets_path: PathBuf) -> anyhow::Result<()> {
        let mut presets = Presets::new();
        presets.load_from_file(&presets_path)?;

        println!("Available Presets:");

        let presets_list = presets.list();
        for preset in presets_list.iter() {
            println!(); // Enter
            println!("{}", preset);
        }
        Ok(())
    }
}

impl Actor for ProviderAgent {
    type Context = Context<Self>;

    fn started(&mut self, context: &mut Context<Self>) {
        IntervalFunc::new(Duration::from_secs(4), Self::schedule_jobs)
            .finish()
            .spawn(context);
    }
}
