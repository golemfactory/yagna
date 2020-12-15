use crate::events::Event;
use crate::execution::{
    GetExeUnit, GetOfferTemplates, Shutdown as ShutdownExecution, TaskRunner, UpdateActivity,
};
use crate::hardware;
use crate::market::provider_market::{OfferKind, Shutdown as MarketShutdown, Unsubscribe};
use crate::market::{CreateOffer, Preset, PresetManager, ProviderMarket};
use crate::payments::{LinearPricingOffer, Payments, PricingOffer};
use crate::startup_config::{FileMonitor, NodeConfig, ProviderConfig, RecvAccount, RunConfig};
use crate::tasks::task_manager::{InitializeTaskManager, TaskManager};

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

use ya_agreement_utils::agreement::TypedArrayPointer;
use ya_agreement_utils::*;
use ya_client::cli::ProviderApi;
use ya_client_model::payment::Account;
use ya_utils_actix::actix_handler::send_message;
use ya_utils_path::SwapSave;

pub struct ProviderAgent {
    globals: GlobalsManager,
    market: Addr<ProviderMarket>,
    runner: Addr<TaskRunner>,
    task_manager: Addr<TaskManager>,
    presets: PresetManager,
    hardware: hardware::Manager,
    accounts: Vec<Account>,
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
    pub account: Option<RecvAccount>,
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
        if node_config.account.is_some() {
            self.account = node_config.account;
        }
        self.save(path)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        Ok(path.swap_save(serde_json::to_string_pretty(self)?)?)
    }
}

impl ProviderAgent {
    pub async fn new(mut args: RunConfig, config: ProviderConfig) -> anyhow::Result<ProviderAgent> {
        let data_dir = config.data_dir.get_or_create()?.as_path().to_path_buf();
        let api = ProviderApi::try_from(&args.api)?;

        log::info!("Loading payment accounts...");
        let accounts: Vec<Account> = api.payment.get_provider_accounts().await?;
        log::info!("Payment accounts: {:#?}", accounts);
        let registry = config.registry()?;
        registry.validate()?;

        // Generate session id from node name and process id to make sure it's unique.
        let name = args
            .node
            .node_name
            .clone()
            .unwrap_or("provider".to_string());
        let session_id = format!("{}-[{}]", name, std::process::id());
        args.market.session_id = session_id.clone();
        args.runner.session_id = session_id;

        let mut globals = GlobalsManager::try_new(&config.globals_file, args.node)?;
        globals.spawn_monitor(&config.globals_file)?;
        let mut presets = PresetManager::load_or_create(&config.presets_file)?;
        presets.spawn_monitor(&config.presets_file)?;
        let mut hardware = hardware::Manager::try_new(&config)?;
        hardware.spawn_monitor(&config.hardware_file)?;

        let market = ProviderMarket::new(api.market, args.market).start();
        let payments = Payments::new(api.activity.clone(), api.payment).start();
        let runner = TaskRunner::new(api.activity, args.runner, registry, data_dir)?.start();
        let task_manager = TaskManager::new(market.clone(), runner.clone(), payments)?.start();

        Ok(ProviderAgent {
            globals,
            market,
            runner,
            task_manager,
            presets,
            hardware,
            accounts,
        })
    }

    async fn create_offers(
        presets: Vec<Preset>,
        node_info: NodeInfo,
        inf_node_info: InfNodeInfo,
        runner: Addr<TaskRunner>,
        market: Addr<ProviderMarket>,
        accounts: Vec<Account>,
    ) -> anyhow::Result<()> {
        if presets.is_empty() {
            return Err(anyhow!("No Presets were selected. Can't create offers."));
        }

        let preset_names = presets.iter().map(|p| &p.name).collect::<Vec<_>>();
        log::debug!("Preset names: {:?}", preset_names);
        let offer_templates = runner.send(GetOfferTemplates(presets.clone())).await??;
        let subnet = &node_info.subnet;

        for preset in presets {
            let pricing_model: Box<dyn PricingOffer> = match preset.pricing_model.as_str() {
                "linear" => Box::new(LinearPricingOffer::default()),
                other => return Err(anyhow!("Unsupported pricing model: {}", other)),
            };
            let mut offer: OfferTemplate = offer_templates
                .get(&preset.name)
                .ok_or_else(|| anyhow!("Offer template not found for preset [{}]", preset.name))?
                .clone();

            let (initial_price, prices) = get_prices(&pricing_model, &preset, &offer)?;
            offer.set_property("golem.com.usage.vector", get_usage_vector_value(&prices));
            offer.add_constraints(Self::build_constraints(subnet.clone())?);

            let com_info = pricing_model.build(&accounts, initial_price, prices)?;
            let name = preset.exeunit_name.clone();
            let exeunit_desc = runner.send(GetExeUnit { name }).await?.map_err(|error| {
                anyhow!(
                    "Failed to create offer for preset [{}]. Error: {}",
                    preset.name,
                    error
                )
            })?;

            let srv_info = ServiceInfo::new(inf_node_info.clone(), exeunit_desc.build())
                .support_multi_activity(true);

            // Create simple offer on market.
            let create_offer_message = CreateOffer {
                preset,
                offer_definition: OfferDefinition {
                    node_info: node_info.clone(),
                    srv_info,
                    com_info,
                    offer,
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

    fn accounts(&self) -> Vec<Account> {
        let globals = self.globals.get_state();
        if let Some(account) = &globals.account {
            let mut accounts = Vec::new();
            if account.platform.is_some() {
                let zkaddr = Account {
                    platform: account.platform.clone().unwrap(),
                    address: account.address.to_lowercase(),
                };
                accounts.push(zkaddr);
            } else {
                for &platform in &["NGNT", "ZK-NGNT"] {
                    accounts.push(Account {
                        platform: platform.to_string(),
                        address: account.address.to_lowercase(),
                    })
                }
            }

            accounts
        } else {
            self.accounts.clone()
        }
    }
}

fn get_prices(
    pricing_model: &Box<dyn PricingOffer>,
    preset: &Preset,
    offer: &OfferTemplate,
) -> Result<(f64, Vec<(String, f64)>), Error> {
    let pointer = offer.property("golem.com.usage.vector");
    let offer_usage_vec = pointer
        .as_typed_array(serde_json::Value::as_str)
        .unwrap_or_else(|_| Vec::new());

    let initial_price = preset
        .get_initial_price()
        .ok_or_else(|| anyhow!("Preset [{}] is missing the initial price", preset.name))?;
    let prices = pricing_model
        .prices(&preset)
        .into_iter()
        .filter_map(|(c, v)| match c.to_property() {
            Some(prop) => match offer_usage_vec.contains(&prop) {
                true => Some((prop.to_string(), v)),
                false => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();

    if prices.is_empty() {
        return Err(anyhow!(
            "Unsupported coefficients [{:?}] in preset {} [{}]",
            preset.usage_coeffs,
            preset.name,
            preset.exeunit_name
        ));
    }

    Ok((initial_price, prices))
}

fn get_usage_vector_value(prices: &Vec<(String, f64)>) -> serde_json::Value {
    let vec = prices
        .iter()
        .map(|(p, _)| serde_json::Value::String(p.clone()))
        .collect::<Vec<_>>();
    serde_json::Value::Array(vec)
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
        let runner = self.runner.clone();

        async move {
            market.send(MarketShutdown).await??;
            runner.send(ShutdownExecution).await??;
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
        let accounts = self.accounts();
        let inf_node_info = InfNodeInfo::from(self.hardware.capped());
        let preset_names = match msg.0 {
            OfferKind::Any => self.presets.active(),
            OfferKind::WithPresets(names) => names,
            OfferKind::WithIds(_) => {
                log::warn!("ProviderAgent shouldn't create Offers using OfferKind::WithIds");
                vec![]
            }
        };

        let presets = self.presets.list_matching(&preset_names);
        async move {
            Self::create_offers(presets?, node_info, inf_node_info, runner, market, accounts).await
        }
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
