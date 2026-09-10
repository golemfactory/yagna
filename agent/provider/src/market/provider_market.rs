use actix::prelude::*;
use actix::AsyncContext;
use anyhow::{anyhow, Error, Result};
use backoff::backoff::Backoff;
use chrono::Utc;
use derive_more::Display;
use futures::prelude::*;
use futures_util::FutureExt;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::time::{timeout, Instant};

use ya_agreement_utils::{AgreementView, OfferDefinition};
use ya_client::market::MarketProviderApi;
use ya_client::model::market::agreement::State as AgreementState;
use ya_client::model::market::agreement_event::AgreementEventType;
use ya_client::model::market::proposal::State;
use ya_client::model::market::{
    agreement_event::AgreementTerminator, Agreement, AgreementTerminationNotice, NewOffer,
    Proposal, ProviderEvent, Reason, Role,
};
use ya_client::model::NodeId;
use ya_std_utils::LogErr;
use ya_utils_actix::{
    actix_handler::ResultTypeGetter, actix_signal::SignalSlot, actix_signal_handler,
    forward_actix_handler,
};

use super::negotiator::factory;
use super::negotiator::{AgreementResponse, AgreementResult, NegotiatorAddr, ProposalResponse};
use super::Preset;
use crate::display::EnableDisplay;
use crate::market::config::MarketConfig;
use crate::market::termination_reason::GolemReason;
use crate::provider_agent::AgentNegotiatorsConfig;
use crate::tasks::task_manager::ClosingCause;
use crate::tasks::{AgreementBroken, AgreementClosed, CloseAgreement};

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

/// Stops accepting new market subscriptions and removes all active Offers.
///
/// The response is sent only after resubscriptions already in flight have
/// either completed and been removed or failed.
#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct BeginDrain;

/// Posts a termination notice for every Approved Agreement of this Provider
/// (best effort), announcing the deadline by which Requestors are expected
/// to finish their work and terminate. Returns the latest announced deadline
/// as a monotonic clock instant. The Agreements stay Approved.
#[derive(Message)]
#[rtype(result = "Result<Instant>")]
pub struct SendTerminationNotices;

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
    result: AgreementResult,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MarketMode {
    Running,
    Draining,
}

struct MarketLifecycle {
    mode: MarketMode,
    pending_resubscriptions: usize,
    resubscription_error: Option<String>,
    drain_waiters: Vec<oneshot::Sender<std::result::Result<(), String>>>,
}

impl Default for MarketLifecycle {
    fn default() -> Self {
        Self {
            mode: MarketMode::Running,
            pending_resubscriptions: 0,
            resubscription_error: None,
            drain_waiters: Vec::new(),
        }
    }
}

impl MarketLifecycle {
    fn is_running(&self) -> bool {
        self.mode == MarketMode::Running
    }

    fn start_resubscription(&mut self) -> bool {
        if !self.is_running() {
            return false;
        }
        self.pending_resubscriptions += 1;
        true
    }

    fn begin_drain(&mut self) -> oneshot::Receiver<std::result::Result<(), String>> {
        if self.is_running() {
            self.mode = MarketMode::Draining;
            self.resubscription_error = None;
        }

        let (tx, rx) = oneshot::channel();
        if self.pending_resubscriptions == 0 {
            let result = self.resubscription_error.clone().map_or(Ok(()), Err);
            let _ = tx.send(result);
        } else {
            self.drain_waiters.push(tx);
        }
        rx
    }

    fn finish_resubscription(&mut self, result: &Result<(), Error>) {
        self.pending_resubscriptions = self
            .pending_resubscriptions
            .checked_sub(1)
            .expect("resubscription counter underflow");

        if self.mode == MarketMode::Draining {
            if let Err(error) = result {
                self.resubscription_error
                    .get_or_insert_with(|| error.to_string());
            }

            if self.pending_resubscriptions == 0 {
                let result = self.resubscription_error.clone().map_or(Ok(()), Err);
                for waiter in self.drain_waiters.drain(..) {
                    let _ = waiter.send(result.clone());
                }
            }
        }
    }
}

/// Manages market api communication and forwards proposal to implementation of market strategy.
// Outputting empty string for logfn macro purposes
#[derive(Display)]
#[display(fmt = "")]
pub struct ProviderMarket {
    negotiator: Arc<NegotiatorAddr>,
    api: Arc<MarketProviderApi>,
    lifecycle: MarketLifecycle,
    subscriptions: HashMap<String, Subscription>,
    postponed_demands: Vec<SubscriptionProposal>,
    config: Arc<MarketConfig>,
    agent_negotiators_cfg: Arc<AgentNegotiatorsConfig>,

    /// External actors can listen on this signal.
    pub agreement_signed_signal: SignalSlot<NewAgreement>,
    pub agreement_terminated_signal: SignalSlot<CloseAgreement>,

    /// Infinite tasks requiring to be killed on shutdown.
    handles: HashMap<String, SpawnHandle>,
}

#[derive(Clone)]
struct AsyncCtx {
    market: Addr<ProviderMarket>,
    config: Arc<MarketConfig>,
    api: Arc<MarketProviderApi>,
    negotiator: Arc<NegotiatorAddr>,
}

impl ProviderMarket {
    // =========================================== //
    // Initialization
    // =========================================== //

    pub fn new(
        api: MarketProviderApi,
        config: MarketConfig,
        agent_negotiators_cfg: AgentNegotiatorsConfig,
    ) -> ProviderMarket {
        ProviderMarket {
            negotiator: Arc::new(NegotiatorAddr::default()),
            api: Arc::new(api),
            lifecycle: MarketLifecycle::default(),
            subscriptions: HashMap::new(),
            postponed_demands: Vec::new(),
            config: Arc::new(config),
            agent_negotiators_cfg: Arc::new(agent_negotiators_cfg),
            agreement_signed_signal: SignalSlot::<NewAgreement>::default(),
            agreement_terminated_signal: SignalSlot::<CloseAgreement>::default(),
            handles: HashMap::new(),
        }
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

    fn remove_subscriptions(
        &mut self,
        offer_kind: OfferKind,
        ctx: &mut Context<Self>,
    ) -> Vec<String> {
        let subscriptions = match offer_kind {
            OfferKind::Any => {
                log::info!("Unsubscribing all active offers");
                std::mem::take(&mut self.subscriptions)
                    .into_keys()
                    .collect::<Vec<_>>()
            }
            OfferKind::WithPresets(preset_names) => {
                let subscriptions = self
                    .subscriptions
                    .iter()
                    .filter_map(|(id, subscription)| {
                        match preset_names.contains(&subscription.preset.name) {
                            true => Some(id.clone()),
                            false => None,
                        }
                    })
                    .collect::<Vec<_>>();

                log::info!("Unsubscribing {} active offer(s)", subscriptions.len());
                subscriptions
            }
            OfferKind::WithIds(subscriptions) => {
                log::info!("Unsubscribing {} offer(s)", subscriptions.len());
                subscriptions
            }
        };

        for id in &subscriptions {
            self.subscriptions.remove(id);
            if let Some(handle) = self.handles.remove(id) {
                ctx.cancel_future(handle);
            }
        }
        subscriptions
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

    market.send(Subscription { id, offer, preset }).await??;
    Ok(())
}

async fn unsubscribe_all(api: Arc<MarketProviderApi>, subscriptions: Vec<String>) -> Result<()> {
    let mut first_error = None;
    for subscription in &subscriptions {
        log::info!("Unsubscribing: {}", subscription);
        if let Err(error) = api.unsubscribe(subscription).await {
            log::error!(
                "Failed to unsubscribe Offer [{}] from the market: {}",
                subscription,
                error
            );
            first_error.get_or_insert_with(|| {
                anyhow!(
                    "failed to unsubscribe Offer [{}] from the market: {}",
                    subscription,
                    error
                )
            });
        }
    }

    if let Some(error) = first_error {
        return Err(error);
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
    demand: &Proposal,
) -> Result<()> {
    let proposal_id = &demand.proposal_id;

    log::info!(
        "Got proposal [{}] from Requestor [{}] for subscription [{}].",
        proposal_id,
        demand.issuer_id,
        subscription.preset.name,
    );

    let prev_proposal = match &demand.prev_proposal_id {
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
            issuer_id: NodeId::from_str("0x000000000000000000000000000000000000000")?, // How to set?
            state: State::Initial,
            timestamp: Utc::now(), // How to set?
            prev_proposal_id: None,
        },
    };

    let action = ctx
        .negotiator
        .react_to_proposal(prev_proposal, demand.clone())
        .await
        .map_err(|e| {
            anyhow!(
                "Negotiator error while processing proposal {}. Error: {}",
                proposal_id,
                e
            )
        })?;

    log::info!(
        "Decided to {} [{}] for subscription [{}].",
        action,
        proposal_id,
        subscription.preset.name
    );

    match action {
        ProposalResponse::CounterProposal { offer } => {
            ctx.api
                .counter_proposal(&offer, &subscription.id, proposal_id)
                .await?;
        }
        ProposalResponse::AcceptProposal => {
            ctx.api
                .counter_proposal(&subscription.offer, &subscription.id, proposal_id)
                .await?;
        }
        ProposalResponse::IgnoreProposal => log::info!("Ignoring proposal {:?}", proposal_id),
        ProposalResponse::RejectProposal { reason, is_final } => {
            if !is_final {
                let sub_dem = SubscriptionProposal {
                    subscription_id: subscription.id.clone(),
                    proposal: demand.clone(),
                };
                log::debug!(
                    "Postponing rejected Proposal [{}] from Requestor [{}]. Reason: {}",
                    demand.proposal_id,
                    demand.issuer_id,
                    reason.display()
                );
                ctx.market.do_send(PostponeDemand(sub_dem));
            }
            ctx.api
                .reject_proposal(&subscription.id, proposal_id, &reason)
                .await?;
        }
    };
    Ok(())
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

    let config = ctx.config;
    let agreement =
        AgreementView::try_from(agreement).map_err(|e| anyhow!("Invalid agreement. Error: {e}"))?;

    let action = ctx
        .negotiator
        .react_to_agreement(&agreement)
        .await
        .map_err(|e| {
            anyhow!(
                "Negotiator error while processing agreement [{}]. Error: {e}",
                agreement.id,
            )
        })?;

    log::info!(
        "Decided to {action} [{}] for subscription [{}].",
        agreement.id,
        subscription.preset.name
    );

    match action {
        AgreementResponse::ApproveAgreement => {
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
                    &agreement.id,
                    Some(config.session_id.clone()),
                    Some(config.agreement_approve_timeout),
                )
                .await;

            if let Err(error) = result {
                // Notify negotiator, that we couldn't approve.
                let msg = AgreementFinalized {
                    id: agreement.id.clone(),
                    result: AgreementResult::ApprovalFailed,
                };
                let _ = ctx.market.send(msg).await;
                return Err(anyhow!(
                    "Failed to approve agreement [{}]. Error: {error}",
                    agreement.id,
                ));
            }

            // We negotiated agreement and here responsibility of ProviderMarket ends.
            // Notify outside world about agreement for further processing.
        }
        AgreementResponse::RejectAgreement { reason, .. } => {
            ctx.api.reject_agreement(&agreement.id, &reason).await?;
        }
    };
    Ok(())
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

#[derive(Message)]
#[rtype(result = "Result<()>")]
struct ReSubscribe(String);

impl Handler<ReSubscribe> for ProviderMarket {
    type Result = ActorResponse<Self, Result<(), Error>>;

    fn handle(&mut self, msg: ReSubscribe, ctx: &mut Self::Context) -> Self::Result {
        if !self.lifecycle.is_running() {
            log::debug!(
                "Ignoring resubscription [{}] during graceful shutdown.",
                msg.0
            );
            return ActorResponse::reply(Ok(()));
        }

        let Some(subscription) = self.subscriptions.remove(&msg.0) else {
            return ActorResponse::reply(Ok(()));
        };

        if let Some(handle) = self.handles.remove(&msg.0) {
            ctx.cancel_future(handle);
        }

        assert!(self.lifecycle.start_resubscription());
        ActorResponse::r#async(
            resubscribe_offer(ctx.address(), self.api.clone(), subscription)
                .into_actor(self)
                .map(|result, myself, _| {
                    myself.lifecycle.finish_resubscription(&result);
                    result
                }),
        )
    }
}

#[derive(Message)]
#[rtype(result = "Result<()>")]
struct PostponeDemand(SubscriptionProposal);

impl Handler<PostponeDemand> for ProviderMarket {
    type Result = ActorResponse<Self, Result<(), Error>>;

    fn handle(&mut self, msg: PostponeDemand, _ctx: &mut Self::Context) -> Self::Result {
        if self.lifecycle.is_running() {
            self.postponed_demands.push(msg.0);
        }
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
            ctx.spawn(collect_agreement_events(actx).into_actor(self)),
        );

        self.negotiator =
            factory::create_negotiator(ctx.address(), &self.config, &self.agent_negotiators_cfg);
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
                .send(BeginDrain)
                .await?
                .map_err(|e| log::warn!("Failed to unsubscribe Offers. {}", e))
                .ok()
                .unwrap_or(());
            Ok(())
        }
        .boxed_local()
    }
}

impl Handler<BeginDrain> for ProviderMarket {
    type Result = ResponseFuture<Result<(), Error>>;

    fn handle(&mut self, _msg: BeginDrain, ctx: &mut Context<Self>) -> Self::Result {
        let was_running = self.lifecycle.is_running();
        let resubscriptions_finished = self.lifecycle.begin_drain();
        if was_running {
            self.postponed_demands.clear();
        }

        let subscriptions = self.remove_subscriptions(OfferKind::Any, ctx);
        let api = self.api.clone();

        async move {
            // Keep processing the Actor mailbox while both remote cleanup and any
            // resubscription started before the state transition are finishing.
            let unsubscribe = unsubscribe_all(api, subscriptions);
            let wait_for_resubscriptions = async move {
                resubscriptions_finished
                    .await
                    .map_err(|_| anyhow!("resubscription drain waiter was dropped"))?
                    .map_err(Error::msg)
            };
            let (unsubscribe_result, resubscription_result) =
                futures::future::join(unsubscribe, wait_for_resubscriptions).await;

            unsubscribe_result?;
            resubscription_result
        }
        .boxed_local()
    }
}

/// How long a single termination notice waits for the Requestor's node
/// to record and acknowledge it.
const TERMINATION_NOTICE_ACK_TIMEOUT: f32 = 10.0;
const TERMINATION_NOTICE_SEND_CONCURRENCY: usize = 16;
/// Ensures the announced wall-clock deadline has passed before the agent sends
/// its graceful-shutdown terminations.
const TERMINATION_DEADLINE_MARGIN: Duration = Duration::from_secs(1);

fn termination_notice_window(grace_period: Duration) -> Result<Duration> {
    grace_period
        .checked_add(Duration::from_secs_f32(TERMINATION_NOTICE_ACK_TIMEOUT))
        .ok_or_else(|| anyhow!("Termination notice grace period is too large"))
}

impl Handler<SendTerminationNotices> for ProviderMarket {
    type Result = ResponseFuture<Result<Instant, Error>>;

    fn handle(&mut self, _msg: SendTerminationNotices, _ctx: &mut Context<Self>) -> Self::Result {
        let api = self.api.clone();
        let grace_period = self.config.termination_notice_grace_period;
        let session_id = self.config.session_id.clone();

        async move {
            // A notice may reach the Requestor at any point during its ACK
            // timeout. Include that budget so every Requestor gets the full
            // configured grace period after a successful delivery.
            let notice_window = termination_notice_window(grace_period)?;
            let chrono_notice_window = chrono::Duration::from_std(notice_window)
                .map_err(|e| anyhow!("Invalid termination notice grace period: {e}"))?;

            let agreements = api
                .list_agreements(Some(AgreementState::Approved), None, None, Some(session_id))
                .await?;
            let agreement_ids = agreements
                .into_iter()
                .filter(|agreement| agreement.role == Role::Provider)
                .map(|agreement| agreement.id)
                .collect::<Vec<_>>();

            if agreement_ids.is_empty() {
                return Ok(Instant::now());
            }

            let deliveries = futures::stream::iter(agreement_ids)
                .map(|agreement_id| {
                    let api = api.clone();
                    async move {
                        // Calculate each deadline when its bounded-concurrency
                        // delivery slot starts. Later batches therefore don't
                        // lose grace time while earlier notices wait for ACKs.
                        let termination_deadline = Utc::now()
                            .checked_add_signed(chrono_notice_window)
                            .ok_or_else(|| anyhow!("Termination notice deadline is too large"))?;
                        let wait_deadline = Instant::now()
                            .checked_add(notice_window)
                            .and_then(|deadline| deadline.checked_add(TERMINATION_DEADLINE_MARGIN))
                            .ok_or_else(|| anyhow!("Termination notice deadline is too large"))?;
                        let notice = AgreementTerminationNotice {
                            termination_deadline,
                            reason: GolemReason {
                                message: "Provider is shutting down".to_string(),
                                code: "Shutdown".to_string(),
                                extra: Default::default(),
                            }
                            .to_client(),
                        };

                        let result = api
                            .post_agreement_termination_notice(
                                &agreement_id,
                                &notice,
                                Some(TERMINATION_NOTICE_ACK_TIMEOUT),
                            )
                            .await;

                        Ok::<_, Error>((agreement_id, termination_deadline, wait_deadline, result))
                    }
                })
                .buffer_unordered(TERMINATION_NOTICE_SEND_CONCURRENCY)
                .collect::<Vec<_>>()
                .await;

            let mut notified = 0usize;
            let mut latest_wait_deadline = Instant::now();
            let mut latest_termination_deadline: Option<chrono::DateTime<Utc>> = None;
            for delivery in deliveries {
                let (agreement_id, termination_deadline, wait_deadline, result) = delivery?;
                // An acknowledgement error does not prove that the Requestor
                // failed to record the notice, so wait for every attempted
                // notice's deadline, not only acknowledged ones.
                latest_wait_deadline = latest_wait_deadline.max(wait_deadline);
                latest_termination_deadline = Some(
                    latest_termination_deadline.map_or(termination_deadline, |latest| {
                        latest.max(termination_deadline)
                    }),
                );

                match result {
                    Ok(()) => notified += 1,
                    Err(e) => log::warn!(
                        "Failed to send termination notice for Agreement [{}]. Error: {}",
                        agreement_id,
                        e
                    ),
                }
            }

            if notified > 0 {
                log::info!(
                    "Sent termination notices for {} Agreement(s). Requestors are expected \
                     to finish and terminate until {}.",
                    notified,
                    latest_termination_deadline.expect("at least one notice was attempted")
                );
            }
            Ok(latest_wait_deadline)
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
                .create_offer(&msg.offer_definition)
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

async fn terminate_agreement(api: Arc<MarketProviderApi>, msg: AgreementFinalized) {
    let id = msg.id;
    let reason = match &msg.result {
        AgreementResult::ClosedByUs => GolemReason::success(),
        AgreementResult::Broken { reason } => GolemReason::new(reason),
        // No need to terminate, because Requestor already did it.
        AgreementResult::ClosedByRequestor => return,
        // No need to terminate since we didn't have Agreement with Requestor.
        AgreementResult::ApprovalFailed => return,
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

async fn resubscribe_offer(
    market: Addr<ProviderMarket>,
    api: Arc<MarketProviderApi>,
    subscription: Subscription,
) -> Result<()> {
    if let Err(error) = api.unsubscribe(&subscription.id).await {
        // A 404 response is the usual reason for resubscription. Failure to remove
        // the expired Offer must not prevent creating its replacement.
        log::warn!(
            "Failed to unsubscribe offer [{}] before resubscription: {}",
            subscription.id,
            error
        );
    }

    let preset_name = subscription.preset.name.clone();
    subscribe(market, api, subscription.offer, subscription.preset)
        .await
        .log_err_msg(&format!(
            "Unable to create subscription for preset {}",
            preset_name,
        ))?;
    Ok(())
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

        if let AgreementResult::ApprovalFailed = &msg.result {
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
                .agreement_finalized(&agreement_id, result)
                .await
                .log_err_msg(&format!(
                    "Negotiator failed while handling agreement [{}] finalize",
                    &agreement_id,
                ))
                .ok();
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

impl Handler<AgreementClosed> for ProviderMarket {
    type Result = ResponseFuture<anyhow::Result<()>>;

    fn handle(&mut self, msg: AgreementClosed, ctx: &mut Context<Self>) -> Self::Result {
        let msg = AgreementFinalized::from(msg);
        let myself = ctx.address();

        async move { myself.send(msg).await? }.boxed_local()
    }
}

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
        let subscriptions = self.remove_subscriptions(msg.0, ctx);
        unsubscribe_all(self.api.clone(), subscriptions).boxed_local()
    }
}

impl Handler<Subscription> for ProviderMarket {
    type Result = ActorResponse<Self, Result<(), Error>>;

    fn handle(&mut self, msg: Subscription, ctx: &mut Context<Self>) -> Self::Result {
        if self.lifecycle.is_running() {
            return ActorResponse::reply(self.on_subscription(msg, ctx));
        }

        let api = self.api.clone();
        let id = msg.id;
        let preset_name = msg.preset.name;
        ActorResponse::r#async(
            async move {
                log::info!(
                    "Removing late subscription [{}] for preset [{}] during graceful shutdown.",
                    id,
                    preset_name
                );
                api.unsubscribe(&id).await?;
                Ok(())
            }
            .into_actor(self),
        )
    }
}

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
        max_elapsed_time: Some(std::time::Duration::from_secs(u64::MAX)),
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
            result: AgreementResult::Broken { reason: msg.reason },
        }
    }
}

impl From<AgreementClosed> for AgreementFinalized {
    fn from(msg: AgreementClosed) -> Self {
        let result = match msg.send_terminate {
            true => AgreementResult::ClosedByUs,
            false => AgreementResult::ClosedByRequestor,
        };

        AgreementFinalized {
            id: msg.agreement_id,
            result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draining_rejects_new_resubscriptions_and_waits_for_started_one() {
        let mut lifecycle = MarketLifecycle::default();
        assert!(lifecycle.start_resubscription());

        let mut finished = lifecycle.begin_drain();
        assert!(!lifecycle.start_resubscription());
        assert!(matches!(
            finished.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));

        lifecycle.finish_resubscription(&Ok(()));
        assert_eq!(finished.try_recv().unwrap(), Ok(()));
    }

    #[test]
    fn draining_reports_late_subscription_cleanup_failure() {
        let mut lifecycle = MarketLifecycle::default();
        assert!(lifecycle.start_resubscription());
        let mut finished = lifecycle.begin_drain();

        lifecycle.finish_resubscription(&Err(anyhow!("late subscription cleanup failed")));

        assert_eq!(
            finished.try_recv().unwrap(),
            Err("late subscription cleanup failed".to_string())
        );
    }

    #[test]
    fn termination_notice_window_includes_ack_budget() {
        assert_eq!(
            termination_notice_window(Duration::from_secs(300)).unwrap(),
            Duration::from_secs(310)
        );
    }
}
