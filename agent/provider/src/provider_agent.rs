use crate::events::Event;
use crate::execution::{GetExeUnit, GetOfferTemplates, TaskRunner, UpdateActivity};
use crate::hardware;
use crate::market::provider_market::{OfferKind, Unsubscribe, UpdateMarket};
use crate::market::{CreateOffer, Preset, PresetManager, ProviderMarket};
use crate::payments::{LinearPricingOffer, Payments, PricingOffer};
use crate::startup_config::{NodeConfig, ProviderConfig, RunConfig};
use crate::task_manager::{InitializeTaskManager, TaskManager};
use actix::prelude::*;
use actix::utils::IntervalFunc;
use anyhow::{anyhow, Error};
use futures::{FutureExt, StreamExt, TryFutureExt};
use std::convert::TryFrom;
use std::time::Duration;
use ya_agreement_utils::agreement::TypedArrayPointer;
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
        let data_dir = config.data_dir.get_or_create()?.as_path().to_path_buf();
        let api = ProviderApi::try_from(&args.api)?;
        let registry = config.registry()?;
        registry.validate()?;

        let mut presets = PresetManager::load_or_create(&config.presets_file)?;
        presets.spawn_monitor(&config.presets_file)?;
        let mut hardware = hardware::Manager::try_new(&config)?;
        hardware.spawn_monitor(&config.hardware_file)?;

        let market = ProviderMarket::new(api.market, "LimitAgreements").start();
        let payments = Payments::new(api.activity.clone(), api.payment).start();
        let runner = TaskRunner::new(api.activity, args.runner_config, registry, data_dir)?.start();
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

            let com_info = pricing_model.build(initial_price, prices)?;
            let name = preset.exeunit_name.clone();
            let exeunit_desc = runner.send(GetExeUnit { name }).await?.map_err(|error| {
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
