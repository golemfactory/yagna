use actix::prelude::*;
use actix::utils::IntervalFunc;
use anyhow::{anyhow, bail};
use chrono::{Utc, DateTime};
use std::convert::TryInto;
use std::path::PathBuf;
use std::time::Duration;

use ya_agreement_utils::{InfNodeInfo, NodeInfo, OfferBuilder, OfferDefinition, ServiceInfo};
use ya_client::cli::ProviderApi;
use ya_utils_actix::{actix_handler::send_message, actix_signal::Subscribe};

use crate::execution::{
    ActivityCreated, ActivityDestroyed, ExeUnitDesc, ExeUnitsRegistry, GetExeUnit,
    InitializeExeUnits, TaskRunner, UpdateActivity,
};
use crate::market::{
    provider_market::{AgreementApproved, OnShutdown, UpdateMarket},
    CreateOffer, Preset, Presets, ProviderMarket,
};
use crate::payments::{LinearPricingOffer, Payments};
use crate::preset_cli::PresetUpdater;
use crate::startup_config::{NodeConfig, PresetNoInteractive, ProviderConfig, RunConfig};



pub struct ProviderAgent {
    market: Addr<ProviderMarket>,
    runner: Addr<TaskRunner>,
    payments: Addr<Payments>,
    node_info: NodeInfo,
}

impl ProviderAgent {
    pub async fn new(args: RunConfig, config: ProviderConfig) -> anyhow::Result<ProviderAgent> {
        let api: ProviderApi = (&args.api).try_into()?;
        let market = ProviderMarket::new(api.market, "AcceptAll").start();
        let runner = TaskRunner::new(api.activity.clone())?.start();
        let payments =
            Payments::new(api.activity, api.payment, &args.node.credit_address).start();

        let node_info = ProviderAgent::create_node_info(&args.node).await;

        let mut provider = ProviderAgent {
            market,
            runner,
            payments,
            node_info,
        };
        provider.initialize(args, config).await?;

        Ok(provider)
    }

    pub async fn initialize(
        &mut self,
        args: RunConfig,
        config: ProviderConfig,
    ) -> anyhow::Result<()> {
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
            file: PathBuf::from(&config.exe_unit_path),
        };
        self.runner.send(msg).await??;

        Ok(self
            .create_offers(args.presets, config, *args.shutdown)
            .await?)
    }

    async fn create_offers(
        &self,
        presets_names: Vec<String>,
        config: ProviderConfig,
        expires: Duration,
    ) -> anyhow::Result<()> {
        log::debug!("Presets names: {:?}", presets_names);

        if presets_names.is_empty() {
            return Err(anyhow!("No Presets were selected. Can't create offers."));
        }

        let presets = Presets::from_file(&config.presets_file)?.list_matching(&presets_names)?;

        // Compute expected shutdown of provider. This time value will be added as constraint
        // to offers to avoid taking offers, that will last longer, than user wants to
        // provider his computing power.
        let expires = Utc::now() + chrono::Duration::from_std(expires)?;
        log::info!(
            "Preparing offers. Provider will take only offers, that expire before {}.",
            expires
        );

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

            let msg = GetExeUnit {
                name: preset.exeunit_name.clone(),
            };
            let exeunit_desc = self.runner.send(msg).await?.map_err(|error| {
                anyhow!(
                    "Failed to create offer for preset [{}]. Error: {}",
                    preset.name,
                    error
                )
            })?;

            // Create simple offer on market.
            let constraints = self.build_constraints(expires)?;
            let create_offer_message = CreateOffer {
                preset,
                offer_definition: OfferDefinition {
                    node_info: self.node_info.clone(),
                    service: Self::create_service_info(&exeunit_desc),
                    com_info,
                    constraints,
                },
            };
            self.market.send(create_offer_message).await??
        }
        Ok(())
    }

    fn build_constraints(&self, expires: DateTime<Utc>) -> anyhow::Result<String> {
        // If user set subnet name, we should add constraint for filtering
        // nodes that didn't set the same name in properties.
        // TODO: Write better constraints building.
        match self.node_info.subnet.clone() {
            Some(subnet) => Ok(format!(
                "(&(golem.node.debug.subnet={})(golem.srv.comp.expiration<{}))",
                subnet,
                expires.timestamp_millis()
            )),
            None => Ok(format!(
                "(golem.srv.comp.expiration<{})",
                expires.timestamp_millis()
            )),
        }
    }

    fn schedule_jobs(&mut self, _ctx: &mut Context<Self>) {
        send_message(self.runner.clone(), UpdateActivity);
        send_message(self.market.clone(), UpdateMarket);
    }

    async fn create_node_info(config: &NodeConfig) -> NodeInfo {
        // TODO: Get node name from identity API.
        let mut node_info = NodeInfo::with_name(&config.node_name);

        // Debug subnet to filter foreign nodes.
        if let Some(subnet) = config.subnet.clone() {
            node_info.with_subnet(subnet.clone());
        }
        node_info
    }

    fn create_service_info(exeunit_desc: &ExeUnitDesc) -> ServiceInfo {
        let inf = InfNodeInfo::new().with_mem(1.0).with_storage(10.0);

        ServiceInfo::new(inf, exeunit_desc.build())
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
            println!("\n{}", exeunit);
        }
        Ok(())
    }

    pub fn list_presets(config: ProviderConfig) -> anyhow::Result<()> {
        let presets = Presets::from_file(&config.presets_file)?;
        println!("Available Presets:");

        for preset in presets.list().iter() {
            println!("\n{}", preset);
        }
        Ok(())
    }

    pub fn list_metrics(_: ProviderConfig) -> anyhow::Result<()> {
        let preset = Preset::default();
        let metrics_names = preset.list_readable_metrics();
        let metrics = preset.list_usage_metrics();

        for (metric, name) in metrics.iter().zip(metrics_names.iter()) {
            println!("{:15}{}", name, metric);
        }
        Ok(())
    }

    pub fn create_preset(
        config: ProviderConfig,
        params: PresetNoInteractive,
    ) -> anyhow::Result<()> {
        let mut presets = Presets::from_file(&config.presets_file)?;
        let registry = ExeUnitsRegistry::from_file(&config.exe_unit_path)?;

        let mut preset = Preset::default();
        preset.name = params
            .preset_name
            .ok_or(anyhow!("Preset name is required."))?;
        preset.exeunit_name = params.exeunit.ok_or(anyhow!("ExeUnit is required."))?;
        preset.pricing_model = params.pricing.unwrap_or("linear".to_string());

        for (name, price) in params.price.iter() {
            preset.update_price(name, *price)?;
        }

        // Validate ExeUnit existence and pricing model.
        registry.find_exeunit(&preset.exeunit_name)?;
        if !(preset.pricing_model == "linear") {
            bail!("Not supported pricing model.")
        }

        presets.add_preset(preset.clone())?;
        presets.save_to_file(&config.presets_file)?;

        println!();
        println!("Preset created:");
        println!("{}", preset);
        Ok(())
    }

    pub fn create_preset_interactive(config: ProviderConfig) -> anyhow::Result<()> {
        let mut presets = Presets::from_file(&config.presets_file)?;
        let registry = ExeUnitsRegistry::from_file(&config.exe_unit_path)?;

        let exeunits = registry
            .list_exeunits()
            .into_iter()
            .map(|desc| desc.name)
            .collect();
        let pricing_models = vec!["linear".to_string()];

        let preset = PresetUpdater::new(Preset::default(), exeunits, pricing_models).interact()?;

        presets.add_preset(preset.clone())?;
        presets.save_to_file(&config.presets_file)?;

        println!();
        println!("Preset created:");
        println!("{}", preset);
        Ok(())
    }

    pub fn remove_preset(config: ProviderConfig, name: String) -> anyhow::Result<()> {
        let mut presets = Presets::from_file(&config.presets_file)?;

        presets.remove_preset(&name)?;
        presets.save_to_file(&config.presets_file)
    }

    pub fn update_preset_interactive(config: ProviderConfig, name: String) -> anyhow::Result<()> {
        let mut presets = Presets::from_file(&config.presets_file)?;
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
        presets.save_to_file(&config.presets_file)?;

        println!();
        println!("Preset updated:");
        println!("{}", preset);
        Ok(())
    }

    pub fn update_preset(
        config: ProviderConfig,
        name: String,
        params: PresetNoInteractive,
    ) -> anyhow::Result<()> {
        let mut presets = Presets::from_file(&config.presets_file)?;
        let registry = ExeUnitsRegistry::from_file(&config.exe_unit_path)?;

        let mut preset = presets.get(&name)?;

        // All values are optional. If not set, previous value will remain.
        preset.name = params.preset_name.unwrap_or(preset.name);
        preset.exeunit_name = params.exeunit.unwrap_or(preset.exeunit_name);
        preset.pricing_model = params.pricing.unwrap_or(preset.pricing_model);

        for (name, price) in params.price.iter() {
            preset.update_price(name, *price)?;
        }

        // Validate ExeUnit existence and pricing model.
        registry.find_exeunit(&preset.exeunit_name)?;
        if !(preset.pricing_model == "linear") {
            bail!("Not supported pricing model.")
        }

        presets.remove_preset(&name)?;
        presets.add_preset(preset.clone())?;
        presets.save_to_file(&config.presets_file)?;

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
