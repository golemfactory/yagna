use actix::prelude::*;
use anyhow::{anyhow, Error, Result};
use derive_more::Display;
use futures::prelude::*;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::sync::Arc;

use ya_agreement_utils::{AgreementView, OfferDefinition};
use ya_client::market::MarketProviderApi;
use ya_client_model::market::{Agreement, NewOffer, Proposal, ProviderEvent, Reason};
use ya_utils_actix::{
    actix_handler::ResultTypeGetter,
    actix_signal::{SignalSlot, Subscribe},
    forward_actix_handler,
};

use super::mock_negotiator::AcceptAllNegotiator;
use super::negotiator::{AgreementResponse, AgreementResult, Negotiator, ProposalResponse};
use super::Preset;
use crate::market::mock_negotiator::LimitAgreementsNegotiator;
use crate::task_manager::{AgreementBroken, AgreementClosed};

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

    /// External actors can listen on this signal.
    pub agreement_signed_signal: SignalSlot<AgreementApproved>,
}

impl ProviderMarket {
    // =========================================== //
    // Initialization
    // =========================================== //

    pub fn new(api: MarketProviderApi, negotiator_type: &str) -> ProviderMarket {
        return ProviderMarket {
            api: Arc::new(api),
            negotiator: create_negotiator(negotiator_type),
            subscriptions: HashMap::new(),
            agreement_signed_signal: SignalSlot::<AgreementApproved>::new(),
        };
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

    pub fn on_subscribe(
        &mut self,
        msg: Subscribe<AgreementApproved>,
        _ctx: &mut Context<Self>,
    ) -> Result<()> {
        self.agreement_signed_signal.on_subscribe(msg);
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

async fn dispatch_events(
    events: Vec<ProviderEvent>,
    market: Addr<ProviderMarket>,
    api: Arc<MarketProviderApi>,
    subscription: Subscription,
) {
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
            dispatch_event(market.clone(), api.clone(), subscription.clone(), event).map_err(
                |error| {
                    log::error!(
                        "Error processing event: {}, subscription_id: {}.",
                        error,
                        subscription.id
                    );
                },
            )
        })
        .collect::<Vec<_>>();

    let _ = future::join_all(dispatch_futures).await;
}

async fn dispatch_event(
    market: Addr<ProviderMarket>,
    api: Arc<MarketProviderApi>,
    subscription: Subscription,
    event: &ProviderEvent,
) -> Result<()> {
    match event {
        ProviderEvent::ProposalEvent { proposal, .. } => {
            process_proposal(market, api, subscription, proposal).await
        }
        ProviderEvent::AgreementEvent { agreement, .. } => {
            process_agreement(market, api, subscription, agreement).await
        }
        _ => unimplemented!(),
    }
}

async fn process_proposal(
    market: Addr<ProviderMarket>,
    api: Arc<MarketProviderApi>,
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

    match market
        .send(GotProposal::new(subscription.clone(), demand.clone()))
        .await?
    {
        Ok(action) => match action {
            ProposalResponse::CounterProposal { offer } => {
                api.counter_proposal(&offer, &subscription.id, proposal_id)
                    .await?;
            }
            ProposalResponse::AcceptProposal => {
                api.counter_proposal(&subscription.offer, &subscription.id, proposal_id)
                    .await?;
            }
            ProposalResponse::IgnoreProposal => log::info!("Ignoring proposal {:?}", proposal_id),
            ProposalResponse::RejectProposal { reason } => {
                api.reject_proposal_with_reason(
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
    market: Addr<ProviderMarket>,
    api: Arc<MarketProviderApi>,
    subscription: Subscription,
    agreement: &Agreement,
) -> Result<()> {
    log::info!(
        "Got agreement [{}] from Requestor [{}] for subscription [{}].",
        agreement.agreement_id,
        agreement.demand.requestor_id,
        subscription.preset.name,
    );

    let agreement = AgreementView::try_from(agreement)
        .map_err(|e| anyhow!("Invalid agreement. Error: {}", e))?;

    let response = market
        .send(GotAgreement::new(subscription, agreement.clone()))
        .await?;
    match response {
        Ok(action) => match action {
            AgreementResponse::ApproveAgreement => {
                // TODO: We should retry approval, but only a few times, than we should
                //       give up since it's better to take another agreement.
                let result = api
                    .approve_agreement(&agreement.agreement_id, None, Some(10.0))
                    .await;

                if let Err(error) = result {
                    // Notify negotiator, that we couldn't approve.
                    let msg = AgreementFinalized {
                        id: agreement.agreement_id.clone(),
                        result: AgreementResult::ApprovalFailed,
                    };
                    let _ = market.send(msg).await;
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

                let _ = market.send(message).await?;
            }
            AgreementResponse::RejectAgreement { reason } => {
                api.reject_agreement(&agreement.agreement_id, reason.map(|r| Reason::new(r)))
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

// Called time-to-time to read events.
async fn run_step(
    market: Addr<ProviderMarket>,
    api: Arc<MarketProviderApi>,
    subscriptions: HashMap<String, Subscription>,
) -> Result<()> {
    let _ = future::join_all(subscriptions.into_iter().map(move |(id, subs)| {
        let api = api.clone();
        let market = market.clone();
        async move {
            match api.collect(&id, Some(2.0), Some(2)).await {
                Err(error) => {
                    log::error!("Can't query market events. Error: {}", error);
                    match error {
                        ya_client::error::Error::HttpStatusCode { code, .. } => {
                            if code.as_u16() == 404 {
                                let _ = market.send(ReSubscribe(id.clone())).await;
                            }
                        }
                        _ => (),
                    }
                }
                Ok(events) => dispatch_events(events, market.clone(), api.clone(), subs).await,
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
}

impl Handler<UpdateMarket> for ProviderMarket {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, _msg: UpdateMarket, ctx: &mut Context<Self>) -> Self::Result {
        let client = self.api.clone();
        let myself = ctx.address();

        let fut = run_step(myself, client, self.subscriptions.clone());
        ActorResponse::r#async(fut.into_actor(self))
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

impl Handler<AgreementFinalized> for ProviderMarket {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: AgreementFinalized, ctx: &mut Context<Self>) -> Self::Result {
        if let Err(error) = self.negotiator.agreement_finalized(&msg.id, msg.result) {
            log::warn!(
                "Negotiator failed while handling agreement [{}] finalize. Error: {}",
                &msg.id,
                error,
            );
        }

        log::info!("Re-subscribing all active offers to get fresh proposals from the Market");

        let myself = ctx.address();
        let subscriptions = std::mem::replace(&mut self.subscriptions, HashMap::new());
        let subscription_ids = subscriptions.keys().cloned().collect::<Vec<_>>();
        let api = self.api.clone();

        let fut = async move {
            if let Err(e) = unsubscribe_all(api.clone(), subscription_ids).await {
                log::warn!("Failed to unsubscribe offers from the market: {:?}", e);
            }

            for (_, sub) in subscriptions {
                let offer = sub.offer;
                let preset = sub.preset;
                let preset_name = preset.name.clone();

                if let Err(e) = subscribe(myself.clone(), api.clone(), offer, preset).await {
                    log::warn!(
                        "Unable to create subscription for preset {:?}: {:?}",
                        preset_name,
                        e
                    );
                }
            }
        };
        ctx.spawn(fut.into_actor(self));

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
forward_actix_handler!(ProviderMarket, Subscribe<AgreementApproved>, on_subscribe);
forward_actix_handler!(ProviderMarket, AgreementApproved, on_agreement_approved);

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
        AgreementFinalized {
            id: msg.agreement_id,
            result: AgreementResult::Closed,
        }
    }
}
