use crate::events::Event;
use crate::execution::{GetExeUnit, TaskRunner, UpdateActivity};
use crate::hardware;
use crate::market::provider_market::{OfferKind, Unsubscribe, UpdateMarket};
use crate::market::{CreateOffer, Preset, PresetManager, ProviderMarket};
use crate::payments::{LinearPricingOffer, Payments};
use crate::preset_cli::PresetUpdater;
use crate::startup_config::{NodeConfig, PresetNoInteractive, ProviderConfig, RunConfig};
use crate::task_manager::{InitializeTaskManager, TaskManager};
use actix::prelude::*;
use actix::utils::IntervalFunc;
use anyhow::{anyhow, bail, Error};
use futures::{FutureExt, StreamExt, TryFutureExt};
use std::convert::TryFrom;
use std::time::Duration;
use ya_agreement_utils::*;
use ya_client::cli::ProviderApi;
use ya_utils_actix::actix_handler::send_message;

pub struct ProviderAgent {
    market: Addr<ProviderMarket>,
    runner: Addr<TaskRunner>,
    task_manager: Addr<TaskManager>,
    node_info: NodeInfo,
    presets: PresetManager,
    hardware: hardware::Manager,
}

impl ProviderAgent {
    pub async fn new(args: RunConfig, config: ProviderConfig) -> anyhow::Result<ProviderAgent> {
        let api = ProviderApi::try_from(&args.api)?;
        let registry = config.registry()?;
        registry.validate()?;

        let mut presets = PresetManager::load_or_create(&config.presets_file)?;
        presets.spawn_monitor(&config.presets_file)?;
        let mut hardware = hardware::Manager::try_new(&config.hardware_file)?;
        hardware.spawn_monitor(&config.hardware_file)?;

        let market = ProviderMarket::new(api.market, "LimitAgreements").start();
        let payments = Payments::new(api.activity.clone(), api.payment).start();
        let runner = TaskRunner::new(api.activity, args.runner_config.clone(), registry)?.start();
        let task_manager = TaskManager::new(market.clone(), runner.clone(), payments)?.start();
        let node_info = ProviderAgent::create_node_info(&args.node).await;

        Ok(ProviderAgent {
            market,
            runner,
            task_manager,
            node_info,
            presets,
            hardware,
        })
    }

    async fn create_offers(
        presets: Vec<Preset>,
        node_info: NodeInfo,
        inf_node_info: InfNodeInfo,
        runner: Addr<TaskRunner>,
        market: Addr<ProviderMarket>,
    ) -> anyhow::Result<()> {
        if presets.is_empty() {
            return Err(anyhow!("No Presets were selected. Can't create offers."));
        }

        let preset_names = presets.iter().map(|p| &p.name).collect::<Vec<_>>();
        log::debug!("Preset names: {:?}", preset_names);

        for preset in presets {
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
            let exeunit_desc = runner.send(msg).await?.map_err(|error| {
                anyhow!(
                    "Failed to create offer for preset [{}]. Error: {}",
                    preset.name,
                    error
                )
            })?;

            // Create simple offer on market.
            let create_offer_message = CreateOffer {
                preset,
                offer_definition: OfferDefinition {
                    node_info: node_info.clone(),
                    service: ServiceInfo::new(inf_node_info.clone(), exeunit_desc.build()),
                    com_info,
                    constraints: Self::build_constraints(node_info.subnet.clone())?,
                },
            };
            market.send(create_offer_message).await??;
        }
        Ok(())
    }

    fn build_constraints(subnet: Option<String>) -> anyhow::Result<String> {
        let mut cnts = constraints!["golem.srv.comp.expiration" > 0,];
        if let Some(subnet) = subnet {
            cnts = cnts.and(constraints!["golem.node.debug.subnet" == subnet,]);
        }
        Ok(cnts.to_string())
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
            log::info!("Using subnet: {}", subnet);
            node_info.with_subnet(subnet.clone());
        }
        node_info
    }
}

// CLI
impl ProviderAgent {
    pub fn list_exeunits(config: ProviderConfig) -> anyhow::Result<()> {
        let registry = config.registry()?;
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
        let presets = PresetManager::load_or_create(&config.presets_file)?;
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
        let mut presets = PresetManager::load_or_create(&config.presets_file)?;
        let registry = config.registry()?;

        let mut preset = Preset::default();
        preset.name = params
            .preset_name
            .ok_or(anyhow!("Preset name is required."))?;
        preset.exeunit_name = params.exe_unit.ok_or(anyhow!("ExeUnit is required."))?;
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
        let mut presets = PresetManager::load_or_create(&config.presets_file)?;
        let registry = config.registry()?;

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
        let mut presets = PresetManager::load_or_create(&config.presets_file)?;
        presets.remove_preset(&name)?;
        presets.save_to_file(&config.presets_file)
    }

    pub fn active_presets(config: ProviderConfig) -> anyhow::Result<()> {
        let presets = PresetManager::load_or_create(&config.presets_file)?;
        for preset in presets.active() {
            println!("\n{}", preset);
        }
        Ok(())
    }

    pub fn activate_preset(config: ProviderConfig, name: String) -> anyhow::Result<()> {
        let mut presets = PresetManager::load_or_create(&config.presets_file)?;
        presets.activate(&name)?;
        presets.save_to_file(&config.presets_file)
    }

    pub fn deactivate_preset(config: ProviderConfig, name: String) -> anyhow::Result<()> {
        let mut presets = PresetManager::load_or_create(&config.presets_file)?;
        presets.deactivate(&name)?;
        presets.save_to_file(&config.presets_file)
    }

    pub fn update_preset_interactive(config: ProviderConfig, name: String) -> anyhow::Result<()> {
        let mut presets = PresetManager::load_or_create(&config.presets_file)?;
        let registry = config.registry()?;

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
        let mut presets = PresetManager::load_or_create(&config.presets_file)?;
        let registry = config.registry()?;

        let mut preset = presets.get(&name)?;

        // All values are optional. If not set, previous value will remain.
        preset.name = params.preset_name.unwrap_or(preset.name);
        preset.exeunit_name = params.exe_unit.unwrap_or(preset.exeunit_name);
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

impl Handler<Initialize> for ProviderAgent {
    type Result = ResponseFuture<Result<(), Error>>;

    fn handle(&mut self, _: Initialize, ctx: &mut Context<Self>) -> Self::Result {
        let market = self.market.clone();
        let agent = ctx.address().clone();
        let preset_state = self.presets.state.clone();
        let rx = futures::stream::select_all(vec![
            self.hardware.event_receiver(),
            self.presets.event_receiver(),
        ]);

        Arbiter::spawn(async move {
            rx.for_each_concurrent(1, |e| async {
                match e {
                    Event::HardwareChanged => {
                        let _ = market
                            .send(Unsubscribe(OfferKind::Any))
                            .map_err(|e| log::error!("Cannot unsubscribe offers: {}", e))
                            .await;
                        let _ = agent
                            .send(CreateOffers(OfferKind::Any))
                            .map_err(|e| log::error!("Cannot create offers: {}", e))
                            .await;
                    }
                    Event::PresetsChanged {
                        presets,
                        updated,
                        removed,
                    } => {
                        let mut new_names = presets.active.clone();
                        {
                            let mut state = preset_state.lock().unwrap();
                            new_names.retain(|n| {
                                if state.active.contains(n) {
                                    if !updated.contains(n) {
                                        return false;
                                    }
                                }
                                true
                            });
                            *state = presets;
                        }

                        let mut to_unsub = updated;
                        to_unsub.extend(removed);

                        if !to_unsub.is_empty() {
                            let _ = market
                                .send(Unsubscribe(OfferKind::WithPresets(to_unsub)))
                                .map_err(|e| log::error!("Cannot unsubscribe offers: {}", e))
                                .await;
                        }
                        if !new_names.is_empty() {
                            let _ = agent
                                .send(CreateOffers(OfferKind::WithPresets(new_names)))
                                .map_err(|e| log::error!("Cannot create offers: {}", e))
                                .await;
                        }
                    }
                    _ => (),
                }
            })
            .await;
        });

        let agent = ctx.address();
        let task_manager = self.task_manager.clone();
        async move {
            task_manager.send(InitializeTaskManager {}).await??;
            agent.send(CreateOffers(OfferKind::Any)).await??;
            Ok(())
        }
        .boxed_local()
    }
}

impl Handler<Shutdown> for ProviderAgent {
    type Result = ResponseFuture<Result<(), Error>>;

    fn handle(&mut self, _: Shutdown, _: &mut Context<Self>) -> Self::Result {
        let market = self.market.clone();
        async move {
            market.send(Unsubscribe(OfferKind::Any)).await??;
            Ok(())
        }
        .boxed_local()
    }
}

impl Handler<CreateOffers> for ProviderAgent {
    type Result = ResponseFuture<Result<(), Error>>;

    #[inline]
    fn handle(&mut self, msg: CreateOffers, _: &mut Context<Self>) -> Self::Result {
        let runner = self.runner.clone();
        let market = self.market.clone();
        let node_info = self.node_info.clone();
        let inf_node_info = InfNodeInfo::from(self.hardware.capped());
        let preset_names = match msg.0 {
            OfferKind::Any => self.presets.active(),
            OfferKind::WithPresets(names) => names,
        };

        let presets = self.presets.list_matching(&preset_names);
        async move { Self::create_offers(presets?, node_info, inf_node_info, runner, market).await }
            .boxed_local()
    }
}

#[derive(Message)]
#[rtype(result = "Result<(), Error>")]
pub struct Initialize;

#[derive(Message)]
#[rtype(result = "Result<(), Error>")]
pub struct Shutdown;

#[derive(Message)]
#[rtype(result = "Result<(), Error>")]
struct CreateOffers(pub OfferKind);
