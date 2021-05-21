use actix::prelude::*;
use anyhow::{anyhow, Error};
use futures::{future, FutureExt, StreamExt, TryFutureExt};
use serde::{Deserialize, Deserializer, Serialize};
use std::convert::TryFrom;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use std::{fs, io};

use ya_agreement_utils::agreement::TypedArrayPointer;
use ya_agreement_utils::*;
use ya_client::cli::ProviderApi;
use ya_core_model::{payment::local::NetworkName, NodeId};
use ya_file_logging::{start_logger, LoggerHandle};
use ya_utils_path::SwapSave;

use crate::dir::clean_provider_dir;
use crate::events::Event;
use crate::execution::{
    GetExeUnit, GetOfferTemplates, Shutdown as ShutdownExecution, TaskRunner, UpdateActivity,
};
use crate::hardware;
use crate::market::provider_market::{OfferKind, Shutdown as MarketShutdown, Unsubscribe};
use crate::market::{CreateOffer, Preset, PresetManager, ProviderMarket};
use crate::payments::{AccountView, LinearPricingOffer, Payments, PricingOffer};
use crate::startup_config::{FileMonitor, NodeConfig, ProviderConfig, RunConfig};
use crate::tasks::task_manager::{InitializeTaskManager, TaskManager};

pub struct ProviderAgent {
    globals: GlobalsManager,
    market: Addr<ProviderMarket>,
    runner: Addr<TaskRunner>,
    task_manager: Addr<TaskManager>,
    presets: PresetManager,
    hardware: hardware::Manager,
    accounts: Vec<AccountView>,
    log_handler: LoggerHandle,
    network: NetworkName,
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

#[derive(Clone, Debug, Default, Serialize, derive_more::Display)]
#[display(
    fmt = "{}{}{}",
    "node_name.as_ref().map(|nn| format!(\"Node name: {}\", nn)).unwrap_or(\"\".into())",
    "subnet.as_ref().map(|s| format!(\"\nSubnet: {}\", s)).unwrap_or(\"\".into())",
    "account.as_ref().map(|a| format!(\"\nAccount: {}\", a)).unwrap_or(\"\".into())"
)]
pub struct GlobalsState {
    pub node_name: Option<String>,
    pub subnet: Option<String>,
    pub account: Option<NodeId>,
}

impl<'de> Deserialize<'de> for GlobalsState {
    fn deserialize<D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, <D as Deserializer<'de>>::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        pub enum Account {
            NodeId(NodeId),
            Deprecated {
                platform: Option<String>,
                address: NodeId,
            },
        }

        impl Account {
            pub fn address(self) -> NodeId {
                match self {
                    Account::NodeId(address) => address,
                    Account::Deprecated { address, .. } => address,
                }
            }
        }

        #[derive(Deserialize)]
        pub struct GenericGlobalsState {
            pub node_name: Option<String>,
            pub subnet: Option<String>,
            pub account: Option<Account>,
        }

        let s = GenericGlobalsState::deserialize(deserializer)?;
        Ok(GlobalsState {
            node_name: s.node_name,
            subnet: s.subnet,
            account: s.account.map(|a| a.address()),
        })
    }
}

impl GlobalsState {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if path.exists() {
            log::debug!("Loading global state from: {}", path.display());
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
        if node_config.node_name.is_some() {
            self.node_name = node_config.node_name;
        }
        if node_config.subnet.is_some() {
            self.subnet = node_config.subnet;
        }
        if node_config.account.account.is_some() {
            self.account = node_config.account.account;
        }
        self.save(path)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        Ok(path.swap_save(serde_json::to_string_pretty(self)?)?)
    }
}

impl ProviderAgent {
    pub async fn new(mut args: RunConfig, config: ProviderConfig) -> anyhow::Result<ProviderAgent> {
        let data_dir = config.data_dir.get_or_create()?;
        
        //log_dir is the same as data_dir by default, but can be changed using --log-dir option       
        let log_dir = if let Some(log_dir) = &config.log_dir {
            log_dir.get_or_create()?
        }
        else {
            data_dir.clone()
        };

        //if --debug option is provided override RUST_LOG flag with debug defaults
        //if you want to more detailed control over logs use RUST_LOG variable and do not use --debug flag
        if args.debug {
            std::env::set_var("RUST_LOG", "debug,tokio_core=info,tokio_reactor=info,hyper=info,reqwest=info");
        }
        //start_logger is using env var RUST_LOG internally
        let log_handler = start_logger("info", Some(&log_dir), &vec![])?;

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
        let name = args.node.node_name.clone().unwrap_or(app_name.to_string());
        args.market.session_id = format!("{}-{}", name, std::process::id());
        args.runner.session_id = args.market.session_id.clone();
        args.payment.session_id = args.market.session_id.clone();

        let network = args.node.account.network.clone();
        let net_color = match network {
            NetworkName::Mainnet => yansi::Color::Magenta,
            NetworkName::Rinkeby => yansi::Color::Cyan,
            _ => yansi::Color::Red,
        };
        log::info!("Using payment network: {}", net_color.paint(&network));
        let mut globals = GlobalsManager::try_new(&config.globals_file, args.node)?;
        globals.spawn_monitor(&config.globals_file)?;
        let mut presets = PresetManager::load_or_create(&config.presets_file)?;
        presets.spawn_monitor(&config.presets_file)?;
        let mut hardware = hardware::Manager::try_new(&config)?;
        hardware.spawn_monitor(&config.hardware_file)?;

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
            network,
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
                "linear" => match std::env::var("DEBIT_NOTE_INTERVAL") {
                    Ok(val) => Box::new(LinearPricingOffer::default().interval(val.parse()?)),
                    Err(_) => Box::new(LinearPricingOffer::default()),
                },
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

    fn accounts(&self, network: &NetworkName) -> anyhow::Result<Vec<AccountView>> {
        let globals = self.globals.get_state();
        if let Some(address) = &globals.account {
            log::info!(
                "Filtering payment accounts by address={} and network={}",
                address,
                network
            );
            let accounts: Vec<AccountView> = self
                .accounts
                .iter()
                .filter(|acc| &acc.address == address && &acc.network == network)
                .cloned()
                .collect();

            if accounts.is_empty() {
                anyhow::bail!(
                    "Payment account {} not initialized. Please run\n\
                    \t`yagna payment init --receiver --network {} --account {}`\n\
                    for all drivers you want to use.",
                    address,
                    network,
                    address,
                )
            }

            Ok(accounts)
        } else {
            log::debug!("Filtering payment accounts by network={}", network);
            let accounts: Vec<AccountView> = self
                .accounts
                .iter()
                // FIXME: this is dirty fix -- we can get more that one address from this filter
                // FIXME: use /me endpoint and filter out only accounts bound to given app-key
                // FIXME: or introduce param to getProviderAccounts to filter out external account above
                .filter(|acc| &acc.network == network)
                .cloned()
                .collect();

            if accounts.is_empty() {
                anyhow::bail!(
                    "Default payment account not initialized. Please run\n\
                    \t`yagna payment init --receiver --network {}`\n\
                    for all drivers you want to use.",
                    network,
                )
            }

            Ok(accounts)
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
        tokio::time::delay_for(delay).await;
    }
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
        let log_handler = self.log_handler.clone();

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
        let accounts = match self.accounts(&self.network) {
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

#[cfg(test)]
mod test {
    use crate::GlobalsState;

    const GLOBALS_JSON_ALPHA_3: &str = r#"
{
  "node_name": "amusing-crate",
  "subnet": "community.3",
  "account": {
    "platform": null,
    "address": "0x979db95461652299c34e15df09441b8dfc4edf7a"
  }
}
"#;

    const GLOBALS_JSON_ALPHA_4: &str = r#"
{
  "node_name": "amusing-crate",
  "subnet": "community.4",
  "account": "0x979db95461652299c34e15df09441b8dfc4edf7a"
}
"#;

    #[test]
    fn deserialize_globals() {
        let mut g3: GlobalsState = serde_json::from_str(GLOBALS_JSON_ALPHA_3).unwrap();
        let g4: GlobalsState = serde_json::from_str(GLOBALS_JSON_ALPHA_4).unwrap();
        assert_eq!(g3.node_name, Some("amusing-crate".into()));
        assert_eq!(g3.node_name, g4.node_name);
        assert_eq!(g3.subnet, Some("community.3".into()));
        assert_eq!(g4.subnet, Some("community.4".into()));
        g3.subnet = Some("community.4".into());
        assert_eq!(
            serde_json::to_string(&g3).unwrap(),
            serde_json::to_string(&g4).unwrap()
        );
        assert_eq!(
            g3.account.unwrap().to_string(),
            g4.account.unwrap().to_string()
        );
    }

    #[test]
    fn deserialize_no_account() {
        let g: GlobalsState = serde_json::from_str(
            r#"
    {
      "node_name": "amusing-crate",
      "subnet": "community.3"
    }
    "#,
        )
        .unwrap();

        assert_eq!(g.node_name, Some("amusing-crate".into()));
        assert_eq!(g.subnet, Some("community.3".into()));
        assert!(g.account.is_none())
    }

    #[test]
    fn deserialize_null_account() {
        let g: GlobalsState = serde_json::from_str(
            r#"
    {
      "node_name": "amusing-crate",
      "subnet": "community.4",
      "account": null
    }
    "#,
        )
        .unwrap();

        assert_eq!(g.node_name, Some("amusing-crate".into()));
        assert_eq!(g.subnet, Some("community.4".into()));
        assert!(g.account.is_none())
    }
}
