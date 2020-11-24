use actix::prelude::*;
use anyhow::{anyhow, Error, Result};
use backoff::backoff::Backoff;
use derive_more::Display;
use futures::prelude::*;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::sync::Arc;

use ya_agreement_utils::{AgreementView, OfferDefinition};
use ya_client::market::MarketProviderApi;
use ya_client_model::market::{
    Agreement, AgreementOperationEvent as AgreementEvent, NewOffer, Proposal, ProviderEvent, Reason,
};
use ya_utils_actix::{
    actix_handler::ResultTypeGetter,
    actix_signal::{SignalSlot, Subscribe},
    forward_actix_handler,
};

use super::mock_negotiator::AcceptAllNegotiator;
use super::negotiator::{AgreementResponse, AgreementResult, Negotiator, ProposalResponse};
use super::Preset;
use crate::market::config::MarketConfig;
use crate::market::mock_negotiator::LimitAgreementsNegotiator;
use crate::market::termination_reason::GolemReason;
use crate::tasks::{AgreementBroken, AgreementClosed, CloseAgreement};
use actix::AsyncContext;
use chrono::Utc;
use ya_client::model::market::ConvertReason;
use ya_client_model::market::agreement_event::AgreementTerminator;

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

/// Collects events from market and runs negotiations.
/// This event should be sent periodically.
#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct UpdateMarket;

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct Unsubscribe(pub OfferKind);

pub enum OfferKind {
    Any,
    WithPresets(Vec<String>),
}

/// Async code emits this event to ProviderMarket, which reacts to it
/// and broadcasts same event to external world.
#[derive(Clone, Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct AgreementApproved {
    pub agreement: AgreementView,
}

// =========================================== //
// Internal messages
// =========================================== //

/// Sent when subscribing offer to the market will be finished.
#[rtype(result = "Result<()>")]
#[derive(Debug, Clone, Message)]
struct Subscription {
    id: String,
    preset: Preset,
    offer: NewOffer,
}

#[derive(Message)]
#[rtype(result = "Result<ProposalResponse>")]
struct GotProposal {
    subscription: Subscription,
    proposal: Proposal,
}

#[derive(Message)]
#[rtype(result = "Result<AgreementResponse>")]
struct GotAgreement {
    subscription: Subscription,
    agreement: AgreementView,
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

/// Manages market api communication and forwards proposal to implementation of market strategy.
// Outputting empty string for logfn macro purposes
#[derive(Display)]
#[display(fmt = "")]
pub struct ProviderMarket {
    negotiator: Box<dyn Negotiator>,
    api: Arc<MarketProviderApi>,
    subscriptions: HashMap<String, Subscription>,
    config: Arc<MarketConfig>,

    /// External actors can listen on this signal.
    pub agreement_signed_signal: SignalSlot<AgreementApproved>,
    pub agreement_terminated_signal: SignalSlot<CloseAgreement>,
}

#[derive(Clone)]
struct AsyncCtx {
    market: Addr<ProviderMarket>,
    config: Arc<MarketConfig>,
    api: Arc<MarketProviderApi>,
}

impl ProviderMarket {
    // =========================================== //
    // Initialization
    // =========================================== //

    pub fn new(api: MarketProviderApi, config: MarketConfig) -> ProviderMarket {
        return ProviderMarket {
            api: Arc::new(api),
            negotiator: create_negotiator(&config.negotiator_type),
            config: Arc::new(config),
            subscriptions: HashMap::new(),
            agreement_signed_signal: SignalSlot::<AgreementApproved>::new(),
            agreement_terminated_signal: SignalSlot::<CloseAgreement>::new(),
        };
    }

    fn async_context(&self, ctx: &mut Context<Self>) -> AsyncCtx {
        AsyncCtx {
            config: self.config.clone(),
            api: self.api.clone(),
            market: ctx.address(),
        }
    }

    fn on_subscription(&mut self, msg: Subscription, _ctx: &mut Context<Self>) -> Result<()> {
        log::info!(
            "Subscribed offer. Subscription id [{}], preset [{}].",
            &msg.id,
            &msg.preset.name
        );

        self.subscriptions.insert(msg.id.clone(), msg);
        Ok(())
    }

    // =========================================== //
    // Market internals - proposals and agreements reactions
    // =========================================== //

    fn on_proposal(
        &mut self,
        msg: GotProposal,
        _ctx: &mut Context<Self>,
    ) -> Result<ProposalResponse> {
        log::debug!(
            "Got proposal event {:?} with state {:?}",
            msg.proposal.proposal_id,
            msg.proposal.state
        );

        let response = self
            .negotiator
            .react_to_proposal(&msg.subscription.offer, &msg.proposal)?;

        log::info!(
            "Decided to {} proposal [{:?}] for subscription [{}].",
            response,
            msg.proposal.proposal_id,
            msg.subscription.preset.name
        );
        Ok(response)
    }

    fn on_agreement(
        &mut self,
        msg: GotAgreement,
        _ctx: &mut Context<Self>,
    ) -> Result<AgreementResponse> {
        log::debug!("Got agreement event {:?}.", msg.agreement.agreement_id,);
        let response = self.negotiator.react_to_agreement(&msg.agreement)?;

        log::info!(
            "Decided to {} agreement [{}] for subscription [{}].",
            response,
            msg.agreement.agreement_id,
            msg.subscription.preset.name
        );
        Ok(response)
    }

    fn on_agreement_approved(
        &mut self,
        msg: AgreementApproved,
        _ctx: &mut Context<Self>,
    ) -> Result<()> {
        log::info!("Got approved agreement [{}].", msg.agreement.agreement_id,);
        // At this moment we only forward agreement to outside world.
        self.agreement_signed_signal.send_signal(AgreementApproved {
            agreement: msg.agreement,
        })
    }

    // =========================================== //
    // Market internals - event subscription
    // =========================================== //

    pub fn on_subscribe_approved(
        &mut self,
        msg: Subscribe<AgreementApproved>,
        _ctx: &mut Context<Self>,
    ) -> Result<()> {
        self.agreement_signed_signal.on_subscribe(msg);
        Ok(())
    }

    pub fn on_subscribe_terminated(
        &mut self,
        msg: Subscribe<CloseAgreement>,
        _ctx: &mut Context<Self>,
    ) -> Result<()> {
        self.agreement_terminated_signal.on_subscribe(msg);
        Ok(())
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
        api.unsubscribe(&subscription).await?;
    }
    Ok(())
}

async fn dispatch_events(ctx: AsyncCtx, events: Vec<ProviderEvent>, subscription: Subscription) {
    if events.len() == 0 {
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

    let _ = future::join_all(dispatch_futures).await;
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
        _ => unimplemented!(),
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

    match ctx
        .market
        .send(GotProposal::new(subscription.clone(), demand.clone()))
        .await?
    {
        Ok(action) => match action {
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
            ProposalResponse::RejectProposal { reason } => {
                ctx.api
                    .reject_proposal_with_reason(
                        &subscription.id,
                        proposal_id,
                        reason.map(|r| Reason::new(r)),
                    )
                    .await?;
            }
        },
        Err(error) => log::error!(
            "Negotiator error while processing proposal {:?}. Error: {}",
            proposal_id,
            error
        ),
    }
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
    let agreement = AgreementView::try_from(agreement)
        .map_err(|e| anyhow!("Invalid agreement. Error: {}", e))?;

    let response = ctx
        .market
        .send(GotAgreement::new(subscription, agreement.clone()))
        .await?;
    match response {
        Ok(action) => match action {
            AgreementResponse::ApproveAgreement => {
                // TODO: We should retry approval, but only a few times, than we should
                //       give up since it's better to take another agreement.
                let result = ctx
                    .api
                    .approve_agreement(
                        &agreement.agreement_id,
                        Some(config.session_id.clone()),
                        Some(config.agreement_approve_timeout),
                    )
                    .await;

                if let Err(error) = result {
                    // Notify negotiator, that we couldn't approve.
                    let msg = AgreementFinalized {
                        id: agreement.agreement_id.clone(),
                        result: AgreementResult::ApprovalFailed,
                    };
                    let _ = ctx.market.send(msg).await;
                    return Err(anyhow!(
                        "Failed to approve agreement [{}]. Error: {}",
                        agreement.agreement_id,
                        error
                    ));
                }

                // We negotiated agreement and here responsibility of ProviderMarket ends.
                // Notify outside world about agreement for further processing.
                let message = AgreementApproved {
                    agreement: agreement.clone(),
                };

                let _ = ctx.market.send(message).await?;
            }
            AgreementResponse::RejectAgreement { reason } => {
                ctx.api
                    .reject_agreement(&agreement.agreement_id, reason.map(|r| Reason::new(r)))
                    .await?;
            }
        },
        Err(error) => log::error!(
            "Negotiator error while processing agreement {}. Error: {}",
            agreement.agreement_id,
            error
        ),
    }
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
                continue;
            }
            Ok(events) => events,
        };

        for event in events {
            match event {
                AgreementEvent::AgreementTerminatedEvent {
                    agreement_id,
                    reason,
                    terminator,
                    event_date,
                    ..
                } => {
                    // Ignore events sent in reaction to termination by us.
                    if terminator == AgreementTerminator::Requestor {
                        // Notify market about termination.
                        let msg = OnAgreementTerminated {
                            id: agreement_id,
                            reason: reason
                                .map(|reason| Reason::from_json_reason(reason).ok())
                                .flatten(),
                        };
                        ctx.market.send(msg).await.ok();
                    }
                    last_timestamp = event_date
                }
                AgreementEvent::AgreementApprovedEvent { event_date, .. }
                | AgreementEvent::AgreementRejectedEvent { event_date, .. }
                | AgreementEvent::AgreementCancelledEvent { event_date, .. } => {
                    last_timestamp = event_date;
                    continue;
                }
            }
        }
    }
}

// Called time-to-time to read events.
async fn run_step(ctx: AsyncCtx, subscriptions: HashMap<String, Subscription>) -> Result<()> {
    let _ = future::join_all(subscriptions.into_iter().map(move |(id, subs)| {
        let ctx = ctx.clone();
        let timeout = ctx.config.negotiation_events_interval;

        async move {
            match ctx.api.collect(&id, Some(timeout), Some(5)).await {
                Err(error) => {
                    log::error!("Can't query market events. Error: {}", error);
                    match error {
                        ya_client::error::Error::HttpStatusCode { code, .. } => {
                            if code.as_u16() == 404 {
                                let _ = ctx.market.send(ReSubscribe(id.clone())).await;
                            }
                        }
                        _ => (),
                    }
                }
                Ok(events) => dispatch_events(ctx.clone(), events, subs).await,
            }
        }
    }))
    .await;

    Ok(())
}

#[derive(Message)]
#[rtype(result = "()")]
struct ReSubscribe(String);

impl Handler<ReSubscribe> for ProviderMarket {
    type Result = ();

    fn handle(&mut self, msg: ReSubscribe, ctx: &mut Self::Context) -> Self::Result {
        let subs_id = msg.0;
        if let Some(subs) = self.subscriptions.get(&subs_id) {
            let offer = subs.offer.clone();
            let api = self.api.clone();
            let _ = ctx.spawn(
                async move {
                    match api.subscribe(&offer).await {
                        Ok(new_subs_id) => Some((subs_id, new_subs_id)),
                        Err(e) => {
                            log::error!("unable to resubscribe {}: {}", subs_id, e);
                            None
                        }
                    }
                }
                .into_actor(self)
                .then(|r, myself, _ctx| {
                    let api = myself.api.clone();
                    let to_unsubscribe = if let Some((old_subs_id, new_subs_id)) = r {
                        if let Some(mut subs) = myself.subscriptions.remove(&old_subs_id) {
                            subs.id = new_subs_id.clone();
                            log::info!("offer [{}] resubscribed as [{}]", old_subs_id, new_subs_id);
                            let _ = myself.subscriptions.insert(new_subs_id, subs);
                            None
                        } else {
                            Some(new_subs_id)
                        }
                    } else {
                        None
                    };
                    async move {
                        if let Some(new_subs_id) = to_unsubscribe {
                            if let Err(e) = api.unsubscribe(&new_subs_id).await {
                                log::warn!("fail to unsubscribe: {}: {}", new_subs_id, e);
                            }
                        }
                    }
                    .into_actor(myself)
                }),
            );
        }
    }
}

// =========================================== //
// Actix stuff
// =========================================== //

impl Actor for ProviderMarket {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        let actx = self.async_context(ctx);
        ctx.address().do_send(UpdateMarket {});
        ctx.spawn(collect_agreement_events(actx).into_actor(self));
    }
}

impl Handler<UpdateMarket> for ProviderMarket {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: UpdateMarket, ctx: &mut Context<Self>) -> Self::Result {
        let actx = self.async_context(ctx);

        let fut = run_step(actx, self.subscriptions.clone())
            .into_actor(self)
            .map(|_, _, ctx| Ok(ctx.address().do_send(msg)));

        ActorResponse::r#async(fut)
    }
}

impl Handler<OnAgreementTerminated> for ProviderMarket {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: OnAgreementTerminated, _ctx: &mut Context<Self>) -> Self::Result {
        let id = msg.id;
        let reason = msg
            .reason
            .map(|msg| msg.message)
            .unwrap_or("Not specified.".to_string());

        log::info!(
            "Requestor terminated agreement [{}]. Reason: {}",
            id,
            reason
        );

        self.agreement_terminated_signal
            .send_signal(CloseAgreement {
                is_terminated: true,
                agreement_id: id.clone(),
            })
            .map_err(|e| {
                log::error!(
                    "Failed to propagate termination info for agreement [{}]. {}",
                    id,
                    e
                )
            })
            .ok();
        ActorResponse::reply(Ok(()))
    }
}

impl Handler<CreateOffer> for ProviderMarket {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: CreateOffer, ctx: &mut Context<Self>) -> Self::Result {
        log::info!(
            "Creating offer for preset [{}] and ExeUnit [{}]. Usage coeffs: {:?}",
            msg.preset.name,
            msg.preset.exeunit_name,
            msg.preset.usage_coeffs
        );

        let offer = match self.negotiator.create_offer(&msg.offer_definition) {
            Ok(offer) => offer,
            Err(error) => {
                log::error!(
                    "Negotiator failed to create offer for preset [{}]. Error: {}",
                    msg.preset.name,
                    error
                );
                return ActorResponse::reply(Err(error));
            }
        };

        let myself = ctx.address();
        let client = self.api.clone();

        log::debug!(
            "Offer created: {}",
            serde_json::to_string_pretty(&offer).unwrap()
        );

        log::info!("Subscribing to events... [{}]", msg.preset.name);

        let future = async move {
            let preset_name = msg.preset.name.clone();
            subscribe(myself, client, offer, msg.preset)
                .await
                .map_err(|error| {
                    log::error!(
                        "Can't subscribe new offer for preset [{}], error: {}",
                        preset_name,
                        error
                    );
                    error
                })
        };

        ActorResponse::r#async(future.into_actor(self))
    }
}

async fn terminate_agreement(api: Arc<MarketProviderApi>, msg: AgreementFinalized) {
    let id = msg.id;
    let reason = match &msg.result {
        AgreementResult::ClosedByUs => GolemReason::success(),
        AgreementResult::Broken { reason } => GolemReason::new(reason),
        // No need to terminate, because Requestor already did it.
        AgreementResult::ClosedByRequestor => return (),
        // No need to terminate since we didn't have Agreement with Requestor.
        AgreementResult::ApprovalFailed => return (),
    };

    log::info!(
        "Terminating agreement [{}]. Reason: [{}] {}",
        &id,
        &reason.code,
        &reason.message,
    );

    let mut repeats = get_backoff();
    while let Err(e) = api.terminate_agreement(&id, Some(reason.clone())).await {
        let delay = match repeats.next_backoff() {
            Some(delay) => delay,
            None => {
                log::error!(
                    "Failed to terminate agreement [{}]. Error: {}. Max time {:#?} elapsed. No more retries.",
                    &id,
                    e,
                    repeats.max_elapsed_time,
                );
                return ();
            }
        };

        log::warn!(
            "Failed to terminate agreement [{}]. Error: {}. Retry after {:#?}",
            &id,
            e,
            &delay,
        );
        tokio::time::delay_for(delay).await;
    }

    log::info!("Agreement [{}] terminated successfully.", &id);
}

async fn resubscribe_offers(
    market: Addr<ProviderMarket>,
    api: Arc<MarketProviderApi>,
    subscriptions: HashMap<String, Subscription>,
) {
    let subscription_ids = subscriptions.keys().cloned().collect::<Vec<_>>();
    if let Err(e) = unsubscribe_all(api.clone(), subscription_ids).await {
        log::warn!("Failed to unsubscribe offers from the market: {:?}", e);
    }

    for (_, sub) in subscriptions {
        let offer = sub.offer;
        let preset = sub.preset;
        let preset_name = preset.name.clone();

        if let Err(e) = subscribe(market.clone(), api.clone(), offer, preset).await {
            log::warn!(
                "Unable to create subscription for preset {:?}: {:?}",
                preset_name,
                e
            );
        }
    }
}

impl Handler<AgreementFinalized> for ProviderMarket {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: AgreementFinalized, ctx: &mut Context<Self>) -> Self::Result {
        if let Err(error) = self.negotiator.agreement_finalized(&msg.id, &msg.result) {
            log::warn!(
                "Negotiator failed while handling agreement [{}] finalize. Error: {}",
                &msg.id,
                error,
            );
        }

        ctx.spawn(terminate_agreement(self.api.clone(), msg).into_actor(self));

        log::info!("Re-subscribing all active offers to get fresh proposals from the Market");

        let subscriptions = std::mem::replace(&mut self.subscriptions, HashMap::new());
        ctx.spawn(
            resubscribe_offers(ctx.address(), self.api.clone(), subscriptions).into_actor(self),
        );

        // Don't forward error.
        ActorResponse::reply(Ok(()))
    }
}

impl Handler<AgreementClosed> for ProviderMarket {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: AgreementClosed, ctx: &mut Context<Self>) -> Self::Result {
        let msg = AgreementFinalized::from(msg);
        let myself = ctx.address().clone();

        ActorResponse::r#async(async move { myself.send(msg).await? }.into_actor(self))
    }
}

impl Handler<AgreementBroken> for ProviderMarket {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: AgreementBroken, ctx: &mut Context<Self>) -> Self::Result {
        let msg = AgreementFinalized::from(msg);
        let myself = ctx.address().clone();

        ActorResponse::r#async(async move { myself.send(msg).await? }.into_actor(self))
    }
}

impl Handler<Unsubscribe> for ProviderMarket {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: Unsubscribe, _ctx: &mut Context<Self>) -> Self::Result {
        let subscriptions = match msg.0 {
            OfferKind::Any => {
                log::info!("Unsubscribing all active offers");
                std::mem::replace(&mut self.subscriptions, HashMap::new())
                    .into_iter()
                    .map(|(k, _)| k)
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
                subs.iter().for_each(|s| {
                    self.subscriptions.remove(s);
                });

                log::info!("Unsubscribing {} active offer(s)", subs.len());
                subs
            }
        };
        let client = self.api.clone();
        ActorResponse::r#async(unsubscribe_all(client, subscriptions).into_actor(self))
    }
}

forward_actix_handler!(ProviderMarket, GotProposal, on_proposal);
forward_actix_handler!(ProviderMarket, GotAgreement, on_agreement);
forward_actix_handler!(ProviderMarket, Subscription, on_subscription);
forward_actix_handler!(
    ProviderMarket,
    Subscribe<AgreementApproved>,
    on_subscribe_approved
);
forward_actix_handler!(
    ProviderMarket,
    Subscribe<CloseAgreement>,
    on_subscribe_terminated
);
forward_actix_handler!(ProviderMarket, AgreementApproved, on_agreement_approved);

fn get_backoff() -> backoff::ExponentialBackoff {
    // TODO: We could have config for Market actor to be able to set at least initial interval.
    let mut backoff = backoff::ExponentialBackoff::default();
    backoff.current_interval = std::time::Duration::from_secs(5);
    backoff.initial_interval = std::time::Duration::from_secs(5);
    backoff.multiplier = 1.5f64;
    backoff.max_interval = std::time::Duration::from_secs(60 * 60);
    backoff.max_elapsed_time = Some(std::time::Duration::from_secs(u64::max_value()));
    backoff
}

// =========================================== //
// Negotiators factory
// =========================================== //

fn create_negotiator(name: &str) -> Box<dyn Negotiator> {
    match name {
        "AcceptAll" => Box::new(AcceptAllNegotiator::new()),
        "LimitAgreements" => Box::new(LimitAgreementsNegotiator::new(1)),
        _ => {
            log::warn!("Unknown negotiator type {}. Using default: AcceptAll", name);
            Box::new(AcceptAllNegotiator::new())
        }
    }
}

// =========================================== //
// Messages creation helpers
// =========================================== //

impl GotProposal {
    fn new(subscription: Subscription, proposal: Proposal) -> Self {
        Self {
            subscription,
            proposal,
        }
    }
}

impl GotAgreement {
    fn new(subscription: Subscription, agreement: AgreementView) -> Self {
        Self {
            subscription,
            agreement,
        }
    }
}

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
