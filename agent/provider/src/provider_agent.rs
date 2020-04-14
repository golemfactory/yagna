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
    CreateOffer, Preset, Presets, ProviderMarket,
};
use crate::payments::{LinearPricingOffer, Payments};
use crate::preset_cli::PresetUpdater;
use crate::startup_config::{ProviderConfig, RunConfig};

pub struct ProviderAgent {
    market: Addr<ProviderMarket>,
    runner: Addr<TaskRunner>,
    payments: Addr<Payments>,
    node_info: NodeInfo,
    service_info: ServiceInfo,
    exe_unit_path: PathBuf,
}

impl ProviderAgent {
    pub async fn new(run_args: RunConfig, config: ProviderConfig) -> anyhow::Result<ProviderAgent> {
        let market = ProviderMarket::new(run_args.market_client()?, "AcceptAll").start();
        let runner = TaskRunner::new(run_args.activity_client()?)?.start();
        let payments = Payments::new(
            run_args.activity_client()?,
            run_args.payment_client()?,
            &run_args.credit_address,
        )
        .start();

        let node_info = ProviderAgent::create_node_info(&run_args).await;
        let service_info = ProviderAgent::create_service_info();

        let mut provider = ProviderAgent {
            market,
            runner,
            payments,
            node_info,
            service_info,
            exe_unit_path: config.exe_unit_path,
        };
        provider.initialize(run_args.presets).await?;

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
        log::debug!("Presets names: {:?}", presets_names);

        if presets_names.is_empty() {
            return Err(anyhow!("No Presets were selected. Can't create offers."));
        }

        // TODO: Hardcoded presets file path.
        let presets = Presets::new()
            .load_from_file(&PathBuf::from("presets.json"))?
            .list_matching(&presets_names)?;

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

    async fn create_node_info(config: &RunConfig) -> NodeInfo {
        // TODO: Get node name from intentity API.
        NodeInfo::with_name(&config.node_name)
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

    pub fn list_exeunits(config: ProviderConfig) -> anyhow::Result<()> {
        let registry = ExeUnitsRegistry::from_file(&config.exe_unit_path)?;
        if let Err(errors) = registry.validate() {
            println!("Encountered errors while checking ExeUnits:\n{}", errors);
        }

        println!("Available ExeUnits:");

        let exeunits = registry.list_exeunits();
        for exeunit in exeunits.iter() {
            println!(); // Enter
            println!("{}", exeunit);
        }
        Ok(())
    }

    pub fn list_presets(_: ProviderConfig, presets_path: PathBuf) -> anyhow::Result<()> {
        let presets = Presets::from_file(&presets_path)?;
        println!("Available Presets:");

        let presets_list = presets.list();
        for preset in presets_list.iter() {
            println!(); // Enter
            println!("{}", preset);
        }
        Ok(())
    }

    pub fn create_preset(config: ProviderConfig, presets_path: PathBuf) -> anyhow::Result<()> {
        let mut presets = Presets::from_file(&presets_path)?;
        let registry = ExeUnitsRegistry::from_file(&config.exe_unit_path)?;

        let exeunits = registry
            .list_exeunits()
            .into_iter()
            .map(|desc| desc.name)
            .collect();
        let pricing_models = vec!["linear".to_string()];

        let preset = PresetUpdater::new(Preset::default(), exeunits, pricing_models).interact()?;

        presets.add_preset(preset.clone())?;
        presets.save_to_file(&presets_path)?;

        println!();
        println!("Preset created:");
        println!("{}", preset);
        Ok(())
    }

    pub fn remove_preset(
        _config: ProviderConfig,
        presets_path: PathBuf,
        name: String,
    ) -> anyhow::Result<()> {
        let mut presets = Presets::from_file(&presets_path)?;

        presets.remove_preset(&name)?;
        presets.save_to_file(&presets_path)
    }

    pub fn update_preset(
        config: ProviderConfig,
        presets_path: PathBuf,
        name: String,
    ) -> anyhow::Result<()> {
        let mut presets = Presets::from_file(&presets_path)?;
        let registry = ExeUnitsRegistry::from_file(&config.exe_unit_path)?;

        let exeunits = registry
            .list_exeunits()
            .into_iter()
            .map(|desc| desc.name)
            .collect();
        let pricing_models = vec!["linear".to_string()];

        let preset =
            PresetUpdater::new(presets.get(&name)?, exeunits, pricing_models).interact()?;

        presets.remove_preset(&name)?;
        presets.add_preset(preset.clone())?;
        presets.save_to_file(&presets_path)?;

        println!();
        println!("Preset updated:");
        println!("{}", preset);
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
