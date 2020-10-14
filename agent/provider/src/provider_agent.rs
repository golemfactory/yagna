use crate::events::Event;
use crate::execution::{GetExeUnit, TaskRunner, UpdateActivity};
use crate::hardware;
use crate::market::provider_market::{OfferKind, Unsubscribe, UpdateMarket};
use crate::market::{CreateOffer, Preset, PresetManager, ProviderMarket};
use crate::payments::{LinearPricingOffer, Payments};
use crate::startup_config::{FileMonitor, NodeConfig, ProviderConfig, RunConfig};
use crate::task_manager::{InitializeTaskManager, TaskManager};
use actix::prelude::*;
use actix::utils::IntervalFunc;
use anyhow::{anyhow, Error};
use futures::{FutureExt, StreamExt, TryFutureExt};
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{fs, io};
use ya_agreement_utils::*;
use ya_client::cli::ProviderApi;
use ya_utils_actix::actix_handler::send_message;
use ya_utils_path::SwapSave;

pub struct ProviderAgent {
    globals: GlobalsManager,
    market: Addr<ProviderMarket>,
    runner: Addr<TaskRunner>,
    task_manager: Addr<TaskManager>,
    presets: PresetManager,
    hardware: hardware::Manager,
}

struct GlobalsManager {
    state: Arc<Mutex<GlobalsState>>,
    monitor: Option<FileMonitor>,
}

impl GlobalsManager {
    fn try_new(globals_file: &Path, node_config: NodeConfig) -> anyhow::Result<Self> {
        let mut state = GlobalsState::load_or_create(globals_file)?;
        state.update_and_save(node_config, globals_file)?;

        Ok(Self {
            state: Arc::new(Mutex::new(state)),
            monitor: None,
        })
    }

    fn spawn_monitor(&mut self, globals_file: &Path) -> anyhow::Result<()> {
        let state = self.state.clone();
        let handler = move |p: PathBuf| match GlobalsState::load(&p) {
            Ok(new_state) => {
                *state.lock().unwrap() = new_state;
            }
            Err(e) => log::warn!("Error updating global configuration from {:?}: {:?}", p, e),
        };
        let monitor = FileMonitor::spawn(globals_file, FileMonitor::on_modified(handler))?;
        self.monitor = Some(monitor);
        Ok(())
    }

    fn get_state(&self) -> GlobalsState {
        self.state.lock().unwrap().clone()
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct GlobalsState {
    pub node_name: String,
    pub subnet: Option<String>,
}

impl GlobalsState {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if path.exists() {
            Ok(serde_json::from_reader(io::BufReader::new(
                fs::OpenOptions::new().read(true).open(path)?,
            ))?)
        } else {
            Ok(Self::default())
        }
    }

    pub fn load_or_create(path: &Path) -> anyhow::Result<Self> {
        if path.exists() {
            Self::load(path)
        } else {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::File::create(&path)?;
            let state = Self::default();
            state.save(path)?;
            Ok(state)
        }
    }

    pub fn update_and_save(&mut self, node_config: NodeConfig, path: &Path) -> anyhow::Result<()> {
        if let Some(node_name) = node_config.node_name {
            self.node_name = node_name;
        }
        if node_config.subnet.is_some() {
            self.subnet = node_config.subnet;
        }
        self.save(path)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        Ok(path.swap_save(serde_json::to_string_pretty(self)?)?)
    }
}

impl ProviderAgent {
    pub async fn new(args: RunConfig, config: ProviderConfig) -> anyhow::Result<ProviderAgent> {
        let data_dir = config.data_dir.get_or_create()?.as_path().to_path_buf();
        let api = ProviderApi::try_from(&args.api)?;
        let registry = config.registry()?;
        registry.validate()?;

        let mut globals = GlobalsManager::try_new(&config.globals_file, args.node)?;
        globals.spawn_monitor(&config.globals_file)?;
        let mut presets = PresetManager::load_or_create(&config.presets_file)?;
        presets.spawn_monitor(&config.presets_file)?;
        let mut hardware = hardware::Manager::try_new(&config)?;
        hardware.spawn_monitor(&config.hardware_file)?;

        let market = ProviderMarket::new(api.market, "LimitAgreements").start();
        let payments = Payments::new(api.activity.clone(), api.payment).start();
        let runner = TaskRunner::new(api.activity, args.runner_config, registry, data_dir)?.start();
        let task_manager = TaskManager::new(market.clone(), runner.clone(), payments)?.start();

        Ok(ProviderAgent {
            globals,
            market,
            runner,
            task_manager,
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
        let mut cnts =
            constraints!["golem.srv.comp.expiration" > chrono::Utc::now().timestamp_millis(),];
        if let Some(subnet) = subnet {
            cnts = cnts.and(constraints!["golem.node.debug.subnet" == subnet,]);
        }
        Ok(cnts.to_string())
    }

    fn schedule_jobs(&mut self, _ctx: &mut Context<Self>) {
        send_message(self.runner.clone(), UpdateActivity);
        send_message(self.market.clone(), UpdateMarket);
    }

    fn create_node_info(&self) -> NodeInfo {
        let globals = self.globals.get_state();

        // TODO: Get node name from identity API.
        let mut node_info = NodeInfo::with_name(globals.node_name);

        // Debug subnet to filter foreign nodes.
        if let Some(subnet) = &globals.subnet {
            log::info!("Using subnet: {}", subnet);
            node_info.with_subnet(subnet.clone());
        }
        node_info
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
        let node_info = self.create_node_info();
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
