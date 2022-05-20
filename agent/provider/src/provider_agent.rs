use actix::prelude::*;
use anyhow::{anyhow, Error};
use futures::{future, FutureExt, StreamExt, TryFutureExt};

use std::convert::TryFrom;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio_stream::wrappers::WatchStream;

use ya_agreement_utils::agreement::TypedArrayPointer;
use ya_agreement_utils::*;
use ya_client::cli::ProviderApi;
use ya_core_model::payment::local::NetworkName;
use ya_file_logging::{start_logger, LoggerHandle};
use ya_manifest_utils::Keystore;

use crate::config::globals::GlobalsState;
use crate::dir::clean_provider_dir;
use crate::events::Event;
use crate::execution::{
    GetExeUnit, GetOfferTemplates, Shutdown as ShutdownExecution, TaskRunner, UpdateActivity,
};
use crate::hardware;
use crate::market::provider_market::{OfferKind, Shutdown as MarketShutdown, Unsubscribe};
use crate::market::{CreateOffer, Preset, PresetManager, ProviderMarket};
use crate::payments::{AccountView, LinearPricingOffer, Payments, PricingOffer};
use crate::startup_config::{
    FileMonitor, FileMonitorConfig, NodeConfig, ProviderConfig, RunConfig,
};
use crate::tasks::task_manager::{InitializeTaskManager, TaskManager};

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

pub struct ProviderAgent {
    globals: GlobalsManager,
    market: Addr<ProviderMarket>,
    runner: Addr<TaskRunner>,
    task_manager: Addr<TaskManager>,
    presets: PresetManager,
    hardware: hardware::Manager,
    accounts: Vec<AccountView>,
    log_handler: LoggerHandle,
    networks: Vec<NetworkName>,
    keystore_monitor: FileMonitor,
}

impl ProviderAgent {
    pub async fn new(mut args: RunConfig, config: ProviderConfig) -> anyhow::Result<ProviderAgent> {
        let data_dir = config.data_dir.get_or_create()?;

        //log_dir is the same as data_dir by default, but can be changed using --log-dir option
        let log_dir = if let Some(log_dir) = &config.log_dir {
            log_dir.get_or_create()?
        } else {
            data_dir.clone()
        };

        //start_logger is using env var RUST_LOG internally.
        //args.debug options sets default logger to debug
        let log_handler = start_logger("info", Some(&log_dir), &[], args.debug)?;

        let app_name = structopt::clap::crate_name!();
        log::info!(
            "Starting {}. Version: {}.",
            structopt::clap::crate_name!(),
            ya_compile_time_utils::version_describe!()
        );
        log::info!("Data directory: {}", data_dir.display());
        log::info!("Log directory: {}", log_dir.display());

        {
            log::info!("Performing disk cleanup...");
            let freed = clean_provider_dir(&data_dir, "30d", false, false)?;
            let human_freed = bytesize::to_string(freed, false);
            log::info!("Freed {} of disk space", human_freed);
        }

        let api = ProviderApi::try_from(&args.api)?;

        log::info!("Loading payment accounts...");
        let accounts: Vec<AccountView> = api
            .payment
            .get_provider_accounts()
            .await?
            .into_iter()
            .map(Into::into)
            .collect();
        log::info!("Payment accounts: {:#?}", accounts);
        let registry = config.registry()?;
        registry.validate()?;
        registry.test_runtimes()?;

        // Generate session id from node name and process id to make sure it's unique.
        let name = args
            .node
            .node_name
            .clone()
            .unwrap_or_else(|| app_name.to_string());

        let keystore_path = data_dir.join(&config.trusted_keys_file);
        let keystore = match Keystore::load(&keystore_path) {
            Ok(store) => {
                log::info!("Trusted key store loaded from {}", keystore_path.display());
                store
            }
            Err(err) => {
                log::info!("Using a new keystore: {}", err);
                Default::default()
            }
        };

        args.market.session_id = format!("{}-{}", name, std::process::id());
        args.runner.session_id = args.market.session_id.clone();
        args.payment.session_id = args.market.session_id.clone();
        args.market
            .negotiator_config
            .composite_config
            .policy_config
            .trusted_keys = Some(keystore.clone());

        let networks = args.node.account.networks.clone();
        for n in networks.iter() {
            let net_color = match n {
                NetworkName::Mainnet => yansi::Color::Magenta,
                NetworkName::Polygon => yansi::Color::Magenta,
                NetworkName::Rinkeby => yansi::Color::Cyan,
                NetworkName::Mumbai => yansi::Color::Cyan,
                NetworkName::Goerli => yansi::Color::Cyan,
                _ => yansi::Color::Red,
            };
            log::info!("Using payment network: {}", net_color.paint(&n));
        }
        let mut globals = GlobalsManager::try_new(&config.globals_file, args.node)?;
        globals.spawn_monitor(&config.globals_file)?;
        let mut presets = PresetManager::load_or_create(&config.presets_file)?;
        presets.spawn_monitor(&config.presets_file)?;
        let mut hardware = hardware::Manager::try_new(&config)?;
        hardware.spawn_monitor(&config.hardware_file)?;
        let keystore_monitor = spawn_keystore_monitor(&config.trusted_keys_file, keystore)?;

        let market = ProviderMarket::new(api.market, args.market).start();
        let payments = Payments::new(api.activity.clone(), api.payment, args.payment).start();
        let runner = TaskRunner::new(api.activity, args.runner, registry, data_dir)?.start();
        let task_manager =
            TaskManager::new(market.clone(), runner.clone(), payments, args.tasks)?.start();

        Ok(ProviderAgent {
            globals,
            market,
            runner,
            task_manager,
            presets,
            hardware,
            accounts,
            log_handler,
            networks,
            keystore_monitor,
        })
    }

    async fn create_offers(
        presets: Vec<Preset>,
        node_info: NodeInfo,
        inf_node_info: InfNodeInfo,
        runner: Addr<TaskRunner>,
        market: Addr<ProviderMarket>,
        accounts: Vec<AccountView>,
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

            let (initial_price, prices) = get_prices(pricing_model.as_ref(), &preset, &offer)?;
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

    fn create_node_info(&self) -> NodeInfo {
        let globals = self.globals.get_state();

        if let Some(subnet) = &globals.subnet {
            log::info!("Using subnet: {}", yansi::Color::Fixed(184).paint(subnet));
        }

        NodeInfo {
            name: globals.node_name,
            subnet: globals.subnet,
            geo_country_code: None,
        }
    }

    fn accounts(&self, networks: &Vec<NetworkName>) -> anyhow::Result<Vec<AccountView>> {
        let globals = self.globals.get_state();
        if let Some(address) = &globals.account {
            log::info!(
                "Filtering payment accounts by address={} and networks={:?}",
                address,
                networks,
            );
            let accounts: Vec<AccountView> = self
                .accounts
                .iter()
                .filter(|acc| &acc.address == address && networks.contains(&acc.network))
                .cloned()
                .collect();

            if accounts.is_empty() {
                anyhow::bail!(
                    "Payment account {} not initialized. Please run\n\
                    \t`yagna payment init --receiver --network {} --account {}`\n\
                    for all drivers you want to use.",
                    address,
                    networks[0],
                    address,
                )
            }

            Ok(accounts)
        } else {
            log::debug!("Filtering payment accounts by networks={:?}", networks);
            let accounts: Vec<AccountView> = self
                .accounts
                .iter()
                // FIXME: this is dirty fix -- we can get more that one address from this filter
                // FIXME: use /me endpoint and filter out only accounts bound to given app-key
                // FIXME: or introduce param to getProviderAccounts to filter out external account above
                .filter(|acc| networks.contains(&acc.network))
                .cloned()
                .collect();

            if accounts.is_empty() {
                anyhow::bail!(
                    "Default payment account not initialized. Please run\n\
                    \t`yagna payment init --receiver --network {}`\n\
                    for all drivers you want to use.",
                    networks[0],
                )
            }

            Ok(accounts)
        }
    }
}

fn get_prices(
    pricing_model: &dyn PricingOffer,
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
        .filter_map(|(prop, v)| match offer_usage_vec.contains(&prop.as_str()) {
            true => Some((prop, v)),
            false => None,
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

fn get_usage_vector_value(prices: &[(String, f64)]) -> serde_json::Value {
    let vec = prices
        .iter()
        .map(|(p, _)| serde_json::Value::String(p.clone()))
        .collect::<Vec<_>>();
    serde_json::Value::Array(vec)
}

async fn process_activity_events(runner: Addr<TaskRunner>) {
    const ZERO: Duration = Duration::from_secs(0);
    const DEFAULT: Duration = Duration::from_secs(4);

    loop {
        let started = SystemTime::now();
        if let Err(error) = runner.send(UpdateActivity).await {
            log::error!("Error processing activity events: {:?}", error);
        }
        let elapsed = SystemTime::now().duration_since(started).unwrap_or(ZERO);
        let delay = DEFAULT.checked_sub(elapsed).unwrap_or(ZERO);
        tokio::time::sleep(delay).await;
    }
}

fn spawn_keystore_monitor<P: AsRef<Path>>(
    path: P,
    keystore: Keystore,
) -> Result<FileMonitor, Error> {
    let handler = move |p: PathBuf| match Keystore::load(&p) {
        Ok(new_keystore) => {
            keystore.replace(new_keystore);
            log::info!("Trusted keystore updated from {}", p.display());
        }
        Err(e) => log::warn!("Error updating trusted keystore from {:?}: {:?}", p, e),
    };
    let monitor = FileMonitor::spawn_with(
        path,
        FileMonitor::on_modified(handler),
        FileMonitorConfig::silent(),
    )?;
    Ok(monitor)
}

impl Actor for ProviderAgent {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        let runner = self.runner.clone();
        ctx.spawn(process_activity_events(runner).into_actor(self));
    }
}

impl Handler<Initialize> for ProviderAgent {
    type Result = ResponseFuture<Result<(), Error>>;

    fn handle(&mut self, _: Initialize, ctx: &mut Context<Self>) -> Self::Result {
        let market = self.market.clone();
        let agent = ctx.address();
        let preset_state = self.presets.state.clone();

        let rx = futures::stream::select_all(vec![
            WatchStream::new(self.hardware.event_receiver()),
            WatchStream::new(self.presets.event_receiver()),
        ]);

        tokio::task::spawn_local(async move {
            rx.for_each(|e| async {
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
        let log_handler = self.log_handler.clone();
        self.keystore_monitor.stop();

        async move {
            market.send(MarketShutdown).await??;
            runner.send(ShutdownExecution).await??;
            log_handler.shutdown();
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
        let accounts = match self.accounts(&self.networks) {
            Ok(acc) => acc,
            Err(e) => return future::err(e).boxed_local(),
        };
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
