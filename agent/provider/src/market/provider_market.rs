use actix::prelude::*;
use actix::AsyncContext;
use anyhow::{anyhow, Error, Result};
use backoff::backoff::Backoff;
use chrono::Utc;
use derive_more::Display;
use futures::prelude::*;
use futures_util::{FutureExt, TryFutureExt};
use serde_yaml;
use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::timeout;

use ya_agreement_utils::AgreementView;
use ya_client::market::MarketProviderApi;
use ya_client::model::market::agreement_event::{AgreementEventType, AgreementTerminator};
use ya_client::model::market::proposal::State;
use ya_client::model::market::{Agreement, NewOffer, Proposal, ProviderEvent, Reason};
use ya_client::model::NodeId;
use ya_client_model::market::NewProposal;
use ya_negotiators::component::AgreementEvent;
use ya_negotiators::factory::NegotiatorsConfig;
use ya_negotiators::{
    factory, AgreementAction, AgreementResult, NegotiatorAddr, NegotiatorCallbacks, ProposalAction,
};

use ya_std_utils::LogErr;
use ya_utils_actix::{
    actix_handler::ResultTypeGetter, actix_signal::SignalSlot, actix_signal_handler,
    forward_actix_handler,
};

use super::Preset;
use crate::display::EnableDisplay;
use crate::market::config::MarketConfig;
use crate::market::negotiator::builtin::manifest::policy_from_env;
use crate::market::negotiator::builtin::*;
use crate::market::termination_reason::{GolemReason, ProviderAgreementResult};
use crate::payments::{InvoiceNotification, ProviderInvoiceEvent};
use crate::provider_agent::AgentNegotiatorsConfig;
use crate::tasks::task_manager::ClosingCause;
use crate::tasks::{AgreementBroken, AgreementClosed, CloseAgreement};
use crate::typed_props::OfferDefinition;

// =========================================== //
// Public exposed messages
// =========================================== //

/// Sends offer to market.
#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct CreateOffer {
    pub offer_definition: OfferDefinition,
    pub preset: Preset,
}

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct Shutdown;

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct Unsubscribe(pub OfferKind);

pub enum OfferKind {
    Any,
    WithPresets(Vec<String>),
    WithIds(Vec<String>),
}

/// Async code emits this event to ProviderMarket, which reacts to it
/// and broadcasts same event to external world.
#[derive(Clone, Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct NewAgreement {
    pub agreement: AgreementView,
}

// =========================================== //
// Internal messages
// =========================================== //

/// Sent when subscribing offer to the market will be finished.
#[derive(Debug, Clone, Message)]
#[rtype(result = "Result<()>")]
struct Subscription {
    id: String,
    preset: Preset,
    offer: NewOffer,
}

#[derive(Message)]
#[rtype(result = "Result<()>")]
struct AgreementFinalized {
    id: String,
    result: ProviderAgreementResult,
}

#[derive(Message)]
#[rtype(result = "Result<()>")]
struct OnAgreementTerminated {
    id: String,
    reason: Option<Reason>,
}

// =========================================== //
// ProviderMarket declaration
// =========================================== //

pub struct SubscriptionProposal {
    pub subscription_id: String,
    pub proposal: Proposal,
}

/// Manages market api communication and forwards proposal to implementation of market strategy.
// Outputting empty string for logfn macro purposes
#[derive(Display)]
#[display(fmt = "")]
pub struct ProviderMarket {
    negotiator: Arc<NegotiatorAddr>,
    api: Arc<MarketProviderApi>,
    subscriptions: HashMap<String, Subscription>,
    postponed_demands: Vec<SubscriptionProposal>,
    config: Arc<MarketConfig>,
    agent_negotiators_cfg: Arc<AgentNegotiatorsConfig>,

    /// External actors can listen on this signal.
    pub agreement_signed_signal: SignalSlot<NewAgreement>,
    pub agreement_terminated_signal: SignalSlot<CloseAgreement>,

    /// Infinite tasks requiring to be killed on shutdown.
    handles: HashMap<String, SpawnHandle>,

    /// Temporary - used only during initialization
    callbacks: Option<NegotiatorCallbacks>,
}

#[derive(Clone)]
struct AsyncCtx {
    market: Addr<ProviderMarket>,
    config: Arc<MarketConfig>,
    api: Arc<MarketProviderApi>,
    negotiator: Arc<NegotiatorAddr>,
}

fn load_negotiators_config(
    data_dir: &Path,
    create_not_existing: bool,
) -> Result<NegotiatorsConfig> {
    // Register ya-provider built-in negotiators
    register_negotiators();

    let path = data_dir.join("negotiators-config.yaml");

    log::info!("Loading negotiators config: {:?}", path);

    Ok(match File::open(&path) {
        Ok(file) => serde_yaml::from_reader(BufReader::new(file))?,
        Err(_) => {
            log::info!("Negotiators config not found. Using env or defaults.");

            let mut negotiator_config = NegotiatorsConfig::default();

            // Add default negotiators.
            negotiator_config
                .negotiators
                .push(expiration::Config::from_env()?);
            negotiator_config
                .negotiators
                .push(max_agreements::Config::from_env()?);
            negotiator_config
                .negotiators
                .push(payment_timeout::Config::from_env()?);
            negotiator_config
                .negotiators
                .push(note_interval::Config::from_env()?);
            negotiator_config.negotiators.push(policy_from_env()?);

            if create_not_existing {
                log::info!("Creating negotiators config at: {:?}", path);

                let content = serde_yaml::to_string(&negotiator_config)?;
                File::create(&path)
                    .map_err(|e| anyhow!("Can't create file: {:?}. Error: {e}", path))?
                    .write_all(content.as_bytes())?;
            }
            negotiator_config
        }
    })
}

impl ProviderMarket {
    // =========================================== //
    // Initialization
    // =========================================== //

    pub async fn new(
        api: MarketProviderApi,
        data_dir: &Path,
        config: MarketConfig,
        agent_negotiators_cfg: AgentNegotiatorsConfig,
    ) -> Result<ProviderMarket> {
        let negotiator_config = load_negotiators_config(data_dir, config.create_negotiators_config)
            .map_err(|e| {
                anyhow!(
                    "Failed to load negotiators config from {}. Error: {e}",
                    data_dir.display(),
                )
            })?;

        let negotiators_workdir = data_dir.join(&config.negotiators_workdir);
        std::fs::create_dir_all(&negotiators_workdir)?;

        let (negotiator, callbacks) = factory::create_negotiator_actor(
            negotiator_config,
            negotiators_workdir,
            config.negotiators_plugins.clone(),
        )
        .await?;

        Ok(ProviderMarket {
            negotiator,
            api: Arc::new(api),
            config: Arc::new(config),
            subscriptions: HashMap::new(),
            postponed_demands: Vec::new(),
            agent_negotiators_cfg: Arc::new(agent_negotiators_cfg),
            agreement_signed_signal: SignalSlot::<NewAgreement>::default(),
            agreement_terminated_signal: SignalSlot::<CloseAgreement>::default(),
            handles: HashMap::new(),
            callbacks: Some(callbacks),
        })
    }

    fn async_context(&self, ctx: &mut Context<Self>) -> AsyncCtx {
        AsyncCtx {
            config: self.config.clone(),
            api: self.api.clone(),
            market: ctx.address(),
            negotiator: self.negotiator.clone(),
        }
    }

    fn on_subscription(&mut self, msg: Subscription, ctx: &mut Context<Self>) -> Result<()> {
        log::info!(
            "Subscribed offer. Subscription id [{}], preset [{}].",
            &msg.id,
            &msg.preset.name
        );

        let actx = self.async_context(ctx);
        let abort_handle =
            ctx.spawn(collect_negotiation_events(actx, msg.clone()).into_actor(self));

        self.handles.insert(msg.id.clone(), abort_handle);
        self.subscriptions.insert(msg.id.clone(), msg);
        Ok(())
    }

    // =========================================== //
    // Market internals - proposals and agreements reactions
    // =========================================== //

    fn on_agreement_approved(&mut self, msg: NewAgreement, _ctx: &mut Context<Self>) -> Result<()> {
        log::info!("Got approved agreement [{}].", msg.agreement.id,);
        // At this moment we only forward agreement to outside world.
        self.agreement_signed_signal.send_signal(msg)
    }
}

async fn subscribe(
    market: Addr<ProviderMarket>,
    api: Arc<MarketProviderApi>,
    offer: NewOffer,
    preset: Preset,
) -> Result<()> {
    let id = api.subscribe(&offer).await?;

    let _ = market.send(Subscription { id, offer, preset }).await?;
    Ok(())
}

async fn unsubscribe_all(api: Arc<MarketProviderApi>, subscriptions: Vec<String>) -> Result<()> {
    for subscription in subscriptions.iter() {
        log::info!("Unsubscribing: {}", subscription);
        api.unsubscribe(subscription).await?;
    }
    Ok(())
}

async fn dispatch_events(ctx: AsyncCtx, events: Vec<ProviderEvent>, subscription: &Subscription) {
    if events.is_empty() {
        return;
    };

    log::debug!(
        "Collected {} market events for subscription [{}]. Processing...",
        events.len(),
        &subscription.preset.name
    );

    let dispatch_futures = events
        .iter()
        .map(|event| {
            dispatch_event(ctx.clone(), subscription.clone(), event).map_err(|error| {
                log::error!(
                    "Error processing event: {}, subscription_id: {}.",
                    error,
                    subscription.id
                );
            })
        })
        .collect::<Vec<_>>();

    let _ = timeout(
        ctx.config.process_market_events_timeout,
        future::join_all(dispatch_futures),
    )
    .await
    .map_err(|_| {
        log::warn!(
            "Timeout while dispatching events for subscription [{}]",
            subscription.preset.name
        )
    });
}

async fn dispatch_event(
    ctx: AsyncCtx,
    subscription: Subscription,
    event: &ProviderEvent,
) -> Result<()> {
    match event {
        ProviderEvent::ProposalEvent { proposal, .. } => {
            process_proposal(ctx, subscription, proposal).await
        }
        ProviderEvent::AgreementEvent { agreement, .. } => {
            process_agreement(ctx, subscription, agreement).await
        }
        ProviderEvent::ProposalRejectedEvent {
            proposal_id,
            reason,
            ..
        } => {
            // TODO: Analyze whether reason is_final and treat final & non-final rejections
            // differently.
            log::info!(
                "Proposal rejected. proposal_id: {}, reason: {:?}",
                proposal_id,
                reason
            );
            Ok(())
        }
        unimplemented_event => {
            log::warn!("Unimplemented event received: {:?}", unimplemented_event);
            Ok(())
        }
    }
}

async fn process_proposal(
    ctx: AsyncCtx,
    subscription: Subscription,
    their: &Proposal,
) -> Result<()> {
    let proposal_id = &their.proposal_id;

    log::info!(
        "Got proposal [{}] from Requestor [{}] for subscription [{}].",
        proposal_id,
        their.issuer_id,
        subscription.preset.name,
    );

    let prev_proposal = match &their.prev_proposal_id {
        Some(prev_proposal_id) => ctx
            .api
            .get_proposal(&subscription.id, prev_proposal_id)
            .await
            .map_err(|e| {
                anyhow!(
                    "Failed to get previous proposal [{}] for Requestor proposal [{}]. {}",
                    prev_proposal_id,
                    proposal_id,
                    e
                )
            })?,
        // It's first Proposal from Requestor, so we have to use our initial Offer.
        None => Proposal {
            properties: subscription.offer.properties.clone(),
            constraints: subscription.offer.constraints.clone(),
            proposal_id: subscription.id.clone(),
            issuer_id: NodeId::default(),
            state: State::Initial,
            timestamp: Utc::now(), // How to set?
            prev_proposal_id: None,
        },
    };

    ctx.negotiator
        .react_to_proposal(&subscription.id, their, &prev_proposal)
        .await
        .map_err(|e| {
            anyhow!(
                "Negotiator error while processing proposal {}. Error: {}",
                proposal_id,
                e
            )
        })
}

async fn process_agreement(
    ctx: AsyncCtx,
    subscription: Subscription,
    agreement: &Agreement,
) -> Result<()> {
    log::info!(
        "Got agreement [{}] from Requestor [{}] for subscription [{}].",
        agreement.agreement_id,
        agreement.demand.requestor_id,
        subscription.preset.name,
    );

    let agreement =
        AgreementView::try_from(agreement).map_err(|e| anyhow!("Invalid agreement. Error: {e}"))?;

    ctx.negotiator
        .react_to_agreement(&subscription.id, &agreement)
        .await
        .map_err(|e| {
            anyhow!(
                "Negotiator error while processing agreement [{}]. Error: {e}",
                agreement.id
            )
        })
}

async fn collect_agreement_events(ctx: AsyncCtx) {
    let session = ctx.config.session_id.clone();
    let timeout = ctx.config.agreement_events_interval;
    let mut last_timestamp = Utc::now();

    loop {
        let events = match ctx
            .api
            .collect_agreement_events(
                Some(timeout),
                Some(&last_timestamp),
                Some(15),
                Some(session.clone()),
            )
            .await
        {
            Err(e) => {
                log::warn!("Can't query agreement events. Error: {}", e);

                // We need to wait after failure, because in most cases it happens immediately
                // and we are spammed with error logs.
                tokio::time::sleep(std::time::Duration::from_secs_f32(timeout)).await;
                continue;
            }
            Ok(events) => events,
        };

        for event in events {
            last_timestamp = event.event_date;
            let agreement_id = event.agreement_id.clone();

            match event.event_type {
                AgreementEventType::AgreementTerminatedEvent {
                    reason, terminator, ..
                } => {
                    // Ignore events sent in reaction to termination by us.
                    if terminator == AgreementTerminator::Requestor {
                        // Notify market about termination.
                        let msg = OnAgreementTerminated {
                            id: agreement_id,
                            reason,
                        };
                        ctx.market.send(msg).await.ok();
                    }
                }
                _ => {
                    log::trace!("Got: {:?}", event);
                    continue;
                }
            }
        }
    }
}

async fn collect_negotiation_events(ctx: AsyncCtx, subscription: Subscription) {
    let ctx = ctx.clone();
    let id = subscription.id.clone();
    let timeout = ctx.config.negotiation_events_interval;

    loop {
        match ctx.api.collect(&id, Some(timeout), Some(5)).await {
            Err(error) => {
                log::warn!("Can't query market events. Error: {}", error);
                match error {
                    ya_client::error::Error::HttpError { code, .. } => {
                        // this causes Offer refresh after its expiration
                        if code.as_u16() == 404 {
                            log::info!("Resubscribing subscription [{}]", id);
                            ctx.market.do_send(ReSubscribe(id.clone()));
                            return;
                        }
                    }
                    _ => {
                        // We need to wait after failure, because in most cases it happens immediately
                        // and we are spammed with error logs.
                        tokio::time::sleep(std::time::Duration::from_secs_f32(timeout)).await;
                    }
                }
            }
            Ok(events) => dispatch_events(ctx.clone(), events, &subscription).await,
        }
    }
}

async fn collect_proposal_decisions(
    ctx: AsyncCtx,
    mut decisions: mpsc::UnboundedReceiver<ProposalAction>,
) {
    while let Some(action) = decisions.recv().await {
        process_proposal_decision(ctx.clone(), action)
            .await
            .map_err(|e| log::error!("Failed to process Proposal decision: {}", e))
            .ok();
    }
}

async fn process_proposal_decision(ctx: AsyncCtx, decision: ProposalAction) -> anyhow::Result<()> {
    // log::info!(
    //     "Decided to {} [{}] for subscription [{}].",
    //     decision,
    //     decision.id(),
    //     subscription.preset.name
    // );

    match decision {
        ProposalAction::CounterProposal {
            id,
            subscription_id,
            proposal,
        } => {
            ctx.api
                .counter_proposal(&proposal, &subscription_id, &id)
                .await?;
        }
        ProposalAction::AcceptProposal {
            id,
            subscription_id,
        } => {
            // Accepting Proposal means, that we counter with the same Proposal, as we
            // sent in previous round.
            let proposal = ctx.api.get_proposal(&subscription_id, &id).await?;
            let proposal = ctx
                .api
                .get_proposal(
                    &subscription_id,
                    &proposal.prev_proposal_id.unwrap_or_else(|| "".to_string()),
                )
                .await?;

            ctx.api
                .counter_proposal(
                    &NewProposal {
                        properties: proposal.properties,
                        constraints: proposal.constraints,
                    },
                    &subscription_id,
                    &id,
                )
                .await?;
        }
        ProposalAction::RejectProposal {
            id,
            subscription_id,
            reason,
        } => {
            ctx.api
                .reject_proposal(&subscription_id, &id, &reason)
                .await?;
        }
    };
    Ok(())
}

async fn collect_agreement_decisions(
    ctx: AsyncCtx,
    mut decisions: mpsc::UnboundedReceiver<AgreementAction>,
) {
    while let Some(action) = decisions.recv().await {
        process_agreement_decision(ctx.clone(), action)
            .await
            .map_err(|e| log::error!("Failed to process Agreement decision: {}", e))
            .ok();
    }
}

async fn process_agreement_decision(
    ctx: AsyncCtx,
    decision: AgreementAction,
) -> anyhow::Result<()> {
    // log::info!(
    //     "Decided to {} [{}] for subscription [{}].",
    //     decision,
    //     decision.id(),
    //     subscription.preset.name
    // );

    let config = ctx.config;
    match decision {
        AgreementAction::ApproveAgreement { id, .. } => {
            let agreement = ctx.api.get_agreement(&id).await?;
            let agreement = AgreementView::try_from(&agreement)?;

            // Prepare Provider for Agreement. We aren't sure here, that approval will
            // succeed, but we are obligated to reserve all promised resources for Requestor,
            // so after `approve_agreement` will return, we are ready to create activities.
            ctx.market
                .send(NewAgreement {
                    agreement: agreement.clone(),
                })
                .await?
                .ok();

            // TODO: We should retry approval, but only a few times, than we should
            //       give up since it's better to take another agreement.
            let result = ctx
                .api
                .approve_agreement(
                    &id,
                    Some(config.session_id.clone()),
                    Some(config.agreement_approve_timeout),
                )
                .await;

            if let Err(error) = result {
                // Notify negotiator, that we couldn't approve.
                let msg = AgreementFinalized {
                    id: id.clone(),
                    result: ProviderAgreementResult::ApprovalFailed,
                };
                let _ = ctx.market.send(msg).await;
                return Err(anyhow!(
                    "Failed to approve agreement [{id}]. Error: {error}",
                ));
            } else {
                ctx.negotiator
                    .agreement_signed(&agreement)
                    .await
                    .log_warn_msg("Failed to send AgreementSigned message to negotiators.")
                    .ok();
            }

            // We negotiated agreement and here responsibility of ProviderMarket ends.
            // Notify outside world about agreement for further processing.
        }
        AgreementAction::RejectAgreement { reason, id, .. } => {
            ctx.api.reject_agreement(&id, &reason).await?;
        }
    };
    Ok(())
}

#[derive(Message)]
#[rtype(result = "Result<()>")]
struct ReSubscribe(String);

impl Handler<ReSubscribe> for ProviderMarket {
    type Result = ActorResponse<Self, Result<(), Error>>;

    fn handle(&mut self, msg: ReSubscribe, ctx: &mut Self::Context) -> Self::Result {
        let to_resubscribe = self
            .subscriptions
            .values()
            .filter(|sub| sub.id == msg.0)
            .cloned()
            .map(|sub| (sub.id.clone(), sub))
            .collect::<HashMap<String, Subscription>>();

        if !to_resubscribe.is_empty() {
            return ActorResponse::r#async(
                resubscribe_offers(ctx.address(), self.api.clone(), to_resubscribe)
                    .into_actor(self)
                    .map(|_, _, _| Ok(())),
            );
        };
        ActorResponse::reply(Ok(()))
    }
}

#[derive(Message)]
#[rtype(result = "Result<()>")]
struct PostponeDemand(SubscriptionProposal);

impl Handler<PostponeDemand> for ProviderMarket {
    type Result = ActorResponse<Self, Result<(), Error>>;

    fn handle(&mut self, msg: PostponeDemand, _ctx: &mut Self::Context) -> Self::Result {
        self.postponed_demands.push(msg.0);
        ActorResponse::reply(Ok(()))
    }
}

// =========================================== //
// Actix stuff
// =========================================== //

impl Actor for ProviderMarket {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        let actx = self.async_context(ctx);

        // Note: There will be no collision with subscription ids stored normally here.
        self.handles.insert(
            "collect-agreement-events".to_string(),
            ctx.spawn(collect_agreement_events(actx.clone()).into_actor(self)),
        );

        if let Some(callbacks) = self.callbacks.take() {
            self.handles.insert(
                "collect-agreement-decisions".to_string(),
                ctx.spawn(
                    collect_agreement_decisions(actx.clone(), callbacks.agreement_channel)
                        .into_actor(self),
                ),
            );

            self.handles.insert(
                "collect-proposal-decisions".to_string(),
                ctx.spawn(
                    collect_proposal_decisions(actx, callbacks.proposal_channel).into_actor(self),
                ),
            );
        }
    }
}

impl Handler<Shutdown> for ProviderMarket {
    type Result = ResponseFuture<Result<(), Error>>;

    fn handle(&mut self, _msg: Shutdown, ctx: &mut Context<Self>) -> Self::Result {
        for (_, handle) in self.handles.drain() {
            ctx.cancel_future(handle);
        }

        let market = ctx.address();
        async move {
            market
                .send(Unsubscribe(OfferKind::Any))
                .await?
                .map_err(|e| log::warn!("Failed to unsubscribe Offers. {}", e))
                .ok()
                .unwrap_or(());
            Ok(())
        }
        .boxed_local()
    }
}

impl Handler<OnAgreementTerminated> for ProviderMarket {
    type Result = anyhow::Result<()>;

    fn handle(&mut self, msg: OnAgreementTerminated, _ctx: &mut Context<Self>) -> Self::Result {
        let id = msg.id;
        let reason = msg
            .reason
            .map(|msg| msg.message)
            .unwrap_or_else(|| "NotSpecified".to_string());

        log::info!(
            "Agreement [{}] terminated by Requestor. Reason: {}",
            &id,
            reason
        );

        self.agreement_terminated_signal
            .send_signal(CloseAgreement {
                cause: ClosingCause::Termination,
                agreement_id: id.clone(),
            })
            .log_err_msg(&format!(
                "Failed to propagate termination info for agreement [{}]",
                id
            ))
            .ok();
        Ok(())
    }
}

impl Handler<CreateOffer> for ProviderMarket {
    type Result = ResponseFuture<Result<(), Error>>;

    fn handle(&mut self, msg: CreateOffer, ctx: &mut Context<Self>) -> Self::Result {
        let ctx = self.async_context(ctx);

        async move {
            log::info!(
                "Creating offer for preset [{}] and ExeUnit [{}]. Usage coeffs: {:?}",
                msg.preset.name,
                msg.preset.exeunit_name,
                msg.preset.usage_coeffs
            );

            let offer = ctx
                .negotiator
                .create_offer(&msg.offer_definition.into_template())
                .await
                .log_err_msg(&format!(
                    "Negotiator failed to create offer for preset [{}]",
                    msg.preset.name,
                ))?;

            log::info!(
                "Offer for preset: {} = {}",
                msg.preset.name,
                offer.display()
            );

            log::info!("Subscribing to events... [{}]", msg.preset.name);

            let preset_name = msg.preset.name.clone();
            subscribe(ctx.market, ctx.api, offer, msg.preset)
                .await
                .log_err_msg(&format!(
                    "Can't subscribe new offer for preset [{}]",
                    preset_name,
                ))?;
            Ok(())
        }
        .boxed_local()
    }
}

/// Terminate Agreement on yagna Daemon.
async fn terminate_agreement(api: Arc<MarketProviderApi>, msg: AgreementFinalized) {
    let id = msg.id;
    let reason = match &msg.result {
        ProviderAgreementResult::ClosedByUs => GolemReason::success(),
        ProviderAgreementResult::BrokenByUs { reason } => GolemReason::new(reason),
        // No need to terminate, because Requestor already did it.
        ProviderAgreementResult::ClosedByRequestor => return,
        ProviderAgreementResult::BrokenByRequestor { .. } => return,
        // No need to terminate since we didn't have Agreement with Requestor.
        ProviderAgreementResult::ApprovalFailed => return,
    };

    log::info!(
        "Terminating agreement [{}]. Reason: [{}] {}",
        &id,
        &reason.code,
        &reason.message,
    );

    let mut repeats = get_backoff();
    while let Err(e) = api.terminate_agreement(&id, &reason.to_client()).await {
        let delay = match repeats.next_backoff() {
            Some(delay) => delay,
            None => {
                log::error!(
                    "Failed to terminate agreement [{}]. Error: {}. Max time {:#?} elapsed. No more retries.",
                    &id,
                    e,
                    repeats.max_elapsed_time,
                );
                return;
            }
        };

        log::warn!(
            "Failed to terminate agreement [{}]. Error: {}. Retry after {:#?}",
            &id,
            e,
            &delay,
        );
        tokio::time::sleep(delay).await;
    }

    log::info!("Agreement [{}] terminated by Provider.", &id);
}

async fn resubscribe_offers(
    market: Addr<ProviderMarket>,
    api: Arc<MarketProviderApi>,
    subscriptions: HashMap<String, Subscription>,
) {
    let subscription_ids = subscriptions.keys().cloned().collect::<Vec<_>>();
    match market
        .send(Unsubscribe(OfferKind::WithIds(subscription_ids)))
        .await
    {
        Err(e) => log::warn!("Failed to unsubscribe offers from the market: {}", e),
        Ok(Err(e)) => log::warn!("Failed to unsubscribe offers from the market: {}", e),
        _ => (),
    }

    for (_, sub) in subscriptions {
        let offer = sub.offer;
        let preset = sub.preset;
        let preset_name = preset.name.clone();

        subscribe(market.clone(), api.clone(), offer, preset)
            .await
            .log_warn_msg(&format!(
                "Unable to create subscription for preset {}",
                preset_name,
            ))
            .ok();
    }
}

async fn renegotiate_demands(
    ctx: AsyncCtx,
    subscriptions: HashMap<String, Subscription>,
    demands: Vec<SubscriptionProposal>,
) {
    for sub_dem in demands {
        log::info!(
            "Re-negotiating Proposal [{}] with [{}].",
            sub_dem.proposal.proposal_id,
            sub_dem.proposal.issuer_id
        );

        let subscription = subscriptions.get(&sub_dem.subscription_id);
        let demand = sub_dem.proposal;
        match subscription {
            None => {
                log::warn!("Subscription not found: {}", sub_dem.subscription_id);
                None
            }
            Some(sub) => process_proposal(ctx.clone(), sub.clone(), &demand)
                .await
                .log_warn_msg(&format!("Unable to process demand: {}", demand.proposal_id))
                .ok(),
        };
    }
}

impl Handler<AgreementFinalized> for ProviderMarket {
    type Result = ResponseActFuture<Self, Result<()>>;

    fn handle(&mut self, msg: AgreementFinalized, ctx: &mut Context<Self>) -> Self::Result {
        let ctx = self.async_context(ctx);
        let agreement_id = msg.id.clone();
        let result = msg.result.clone();

        if let ProviderAgreementResult::ApprovalFailed = &msg.result {
            self.agreement_terminated_signal
                .send_signal(CloseAgreement {
                    cause: ClosingCause::ApprovalFail,
                    agreement_id: agreement_id.clone(),
                })
                .log_err_msg(&format!(
                    "Failed to propagate ApprovalFailed info for agreement [{}]",
                    agreement_id
                ))
                .ok();
        }

        let async_ctx = ctx.clone();
        let future = async move {
            ctx.negotiator
                .agreement_finalized(&agreement_id, result.try_into()?)
                .await
                .log_err_msg(&format!(
                    "Negotiator failed while handling agreement [{}] finalize.",
                    &agreement_id,
                ))
                .ok();
            ctx.negotiator
                .request_agreements(1)
                .await
                .log_err_msg("Failed to request new Agreement from Negotiator.")
                .ok();
            anyhow::Result::<()>::Ok(())
        }
        .into_actor(self)
        .map(|_, myself, ctx| {
            ctx.spawn(terminate_agreement(myself.api.clone(), msg).into_actor(myself));

            log::info!("Re-negotiating all demands");

            let demands = std::mem::take(&mut myself.postponed_demands);
            ctx.spawn(
                renegotiate_demands(async_ctx, myself.subscriptions.clone(), demands)
                    .into_actor(myself),
            );
            Ok(())
        });
        Box::pin(future)
    }
}

/// Market handles closed Agreement the same way as Broken Agreement, so we
/// translate event to AgreementFinalized and send to ourselves.
impl Handler<AgreementClosed> for ProviderMarket {
    type Result = ResponseFuture<anyhow::Result<()>>;

    fn handle(&mut self, msg: AgreementClosed, ctx: &mut Context<Self>) -> Self::Result {
        let msg = AgreementFinalized::from(msg);
        let myself = ctx.address();

        async move { myself.send(msg).await? }.boxed_local()
    }
}

/// Market handles closed Agreement the same way as Broken Agreement, so we
/// translate event to AgreementFinalized and send to ourselves.
impl Handler<AgreementBroken> for ProviderMarket {
    type Result = ResponseFuture<anyhow::Result<()>>;

    fn handle(&mut self, msg: AgreementBroken, ctx: &mut Context<Self>) -> Self::Result {
        let msg = AgreementFinalized::from(msg);
        let myself = ctx.address();

        async move { myself.send(msg).await? }.boxed_local()
    }
}

impl Handler<Unsubscribe> for ProviderMarket {
    type Result = ResponseFuture<Result<(), Error>>;

    fn handle(&mut self, msg: Unsubscribe, ctx: &mut Context<Self>) -> Self::Result {
        let subscriptions = match msg.0 {
            OfferKind::Any => {
                log::info!("Unsubscribing all active offers");
                std::mem::take(&mut self.subscriptions)
                    .into_keys()
                    .collect::<Vec<_>>()
            }
            OfferKind::WithPresets(preset_names) => {
                let subs = self
                    .subscriptions
                    .iter()
                    .filter_map(|(n, sub)| match preset_names.contains(&sub.preset.name) {
                        true => Some(n.clone()),
                        false => None,
                    })
                    .collect::<Vec<_>>();

                log::info!("Unsubscribing {} active offer(s)", subs.len());
                subs
            }
            OfferKind::WithIds(subs) => {
                log::info!("Unsubscribing {} offer(s)", subs.len());
                subs
            }
        };

        subscriptions.iter().for_each(|id| {
            self.subscriptions.remove(id);
        });
        subscriptions
            .iter()
            .filter_map(|id| self.handles.remove(id))
            .for_each(|handle| {
                ctx.cancel_future(handle);
            });

        unsubscribe_all(self.api.clone(), subscriptions).boxed_local()
    }
}

/// If we get Invoice notification, we should pass it to negotiators.
impl Handler<InvoiceNotification> for ProviderMarket {
    type Result = ResponseFuture<Result<(), Error>>;

    fn handle(&mut self, msg: InvoiceNotification, _ctx: &mut Context<Self>) -> Self::Result {
        let event = match msg.event {
            ProviderInvoiceEvent::InvoiceAcceptedEvent => AgreementEvent::InvoiceAccepted,
            ProviderInvoiceEvent::InvoiceRejectedEvent => AgreementEvent::InvoiceRejected,
            ProviderInvoiceEvent::InvoiceSettledEvent => AgreementEvent::InvoicePaid,
        };

        let negotiator = self.negotiator.clone();

        async move {
            negotiator
                .post_agreement_event(&msg.agreement_id, event)
                .await
                .log_err_msg("Negotiators failed to handle Post Agreement event.")
        }
        .boxed_local()
    }
}

forward_actix_handler!(ProviderMarket, Subscription, on_subscription);
forward_actix_handler!(ProviderMarket, NewAgreement, on_agreement_approved);
actix_signal_handler!(ProviderMarket, CloseAgreement, agreement_terminated_signal);
actix_signal_handler!(ProviderMarket, NewAgreement, agreement_signed_signal);

fn get_backoff() -> backoff::ExponentialBackoff {
    // TODO: We could have config for Market actor to be able to set at least initial interval.
    backoff::ExponentialBackoff {
        current_interval: std::time::Duration::from_secs(5),
        initial_interval: std::time::Duration::from_secs(5),
        multiplier: 1.5f64,
        max_interval: std::time::Duration::from_secs(60 * 60),
        max_elapsed_time: Some(std::time::Duration::from_secs(u64::max_value())),
        ..Default::default()
    }
}

// =========================================== //
// Messages creation helpers
// =========================================== //

impl From<AgreementBroken> for AgreementFinalized {
    fn from(msg: AgreementBroken) -> Self {
        AgreementFinalized {
            id: msg.agreement_id,
            result: ProviderAgreementResult::BrokenByUs { reason: msg.reason },
        }
    }
}

impl From<AgreementClosed> for AgreementFinalized {
    fn from(msg: AgreementClosed) -> Self {
        let result = match msg.send_terminate {
            true => ProviderAgreementResult::ClosedByUs,
            false => ProviderAgreementResult::ClosedByRequestor,
        };

        AgreementFinalized {
            id: msg.agreement_id,
            result,
        }
    }
}

impl TryInto<AgreementResult> for ProviderAgreementResult {
    type Error = anyhow::Error;

    fn try_into(self) -> anyhow::Result<AgreementResult> {
        match self {
            ProviderAgreementResult::ApprovalFailed => Err(anyhow!(
                "AgreementResult::ApprovalFailed can't be translated"
            )),
            ProviderAgreementResult::ClosedByUs => Ok(AgreementResult::ClosedByUs),
            ProviderAgreementResult::ClosedByRequestor => Ok(AgreementResult::ClosedByThem),
            ProviderAgreementResult::BrokenByUs { reason } => Ok(AgreementResult::BrokenByUs {
                reason: GolemReason::new(&reason).to_client(),
            }),
            ProviderAgreementResult::BrokenByRequestor { reason } => {
                Ok(AgreementResult::BrokenByThem { reason })
            }
        }
    }
}
