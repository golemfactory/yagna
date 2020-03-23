use actix::prelude::*;
use anyhow::{Error, Result};
use derive_more::Display;
use futures::future::join_all;
use log_derive::{logfn, logfn_inputs};
use std::collections::HashMap;
use std::sync::Arc;

use ya_client::market::MarketProviderApi;
use ya_model::market::{Agreement, Offer, Proposal, ProviderEvent};
use ya_utils_actix::{
    actix_handler::ResultTypeGetter,
    actix_signal::{SignalSlot, Subscribe},
    forward_actix_handler,
};

use super::mock_negotiator::AcceptAllNegotiator;
use super::negotiator::{AgreementResponse, Negotiator, ProposalResponse};

// Temporrary
use ya_agent_offer_model::OfferDefinition;

// =========================================== //
// Public exposed messages
// =========================================== //

/// Sends offer to market.
#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct CreateOffer {
    pub offer_definition: OfferDefinition,
}

/// Collects events from market and runs negotiations.
/// This event should be sent periodically.
#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct UpdateMarket;

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct OnShutdown;

// =========================================== //
// Internal messages
// =========================================== //

/// Send when subscribing to market will be finished.
#[rtype(result = "Result<()>")]
#[derive(Clone, Debug, Message)]
pub struct OfferSubscription {
    subscription_id: String,
    offer: Offer,
}

#[derive(Message, Debug)]
#[rtype(result = "Result<ProposalResponse>")]
pub struct GotProposal {
    subscription: OfferSubscription,
    proposal: Proposal,
}

impl GotProposal {
    fn new(subscription: OfferSubscription, proposal: Proposal) -> Self {
        Self {
            subscription,
            proposal,
        }
    }
}

#[derive(Message, Debug)]
#[rtype(result = "Result<AgreementResponse>")]
pub struct GotAgreement {
    subscription: OfferSubscription,
    agreement: Agreement,
}

impl GotAgreement {
    fn new(subscription: OfferSubscription, agreement: Agreement) -> Self {
        Self {
            subscription,
            agreement,
        }
    }
}

/// Async code emits this event to ProviderMarket, which reacts to it
/// and broadcasts same event to external world.
#[derive(Clone, Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct AgreementApproved {
    pub agreement: Agreement,
}

// =========================================== //
// ProviderMarket declaration
// =========================================== //

/// Manages market api communication and forwards proposal to implementation of market strategy.
// Outputing empty string for logfn macro purposes
#[derive(Display)]
#[display(fmt = "")]
pub struct ProviderMarket {
    negotiator: Box<dyn Negotiator>,
    market_api: Arc<MarketProviderApi>,
    offer_subscriptions: HashMap<String, OfferSubscription>,

    /// External actors can listen on this signal.
    pub agreement_signed_signal: SignalSlot<AgreementApproved>,
}

impl ProviderMarket {
    // =========================================== //
    // Initialization
    // =========================================== //

    pub fn new(market_api: MarketProviderApi, negotiator_type: &str) -> ProviderMarket {
        return ProviderMarket {
            market_api: Arc::new(market_api),
            negotiator: create_negotiator(negotiator_type),
            offer_subscriptions: HashMap::new(),
            agreement_signed_signal: SignalSlot::<AgreementApproved>::new(),
        };
    }

    async fn create_offer(
        addr: Addr<ProviderMarket>,
        market_api: Arc<MarketProviderApi>,
        offer: Offer,
    ) -> Result<()> {
        let subscription_id = market_api.subscribe(&offer).await?;
        let sub = OfferSubscription {
            subscription_id,
            offer,
        };

        let _ = addr.send(sub).await?;
        Ok(())
    }

    #[logfn_inputs(Debug, fmt = "{}Subscribed offer: {:?} {:?}")]
    fn on_offer_subscribed(&mut self, msg: OfferSubscription, _ctx: &mut Context<Self>) -> Result<()> {
        log::info!(
            "Subscribed offer. Subscription id [{}].",
            &msg.subscription_id
        );

        self.offer_subscriptions
            .insert(msg.subscription_id.clone(), msg);
        Ok(())
    }

    async fn on_shutdown(
        market_api: Arc<MarketProviderApi>,
        subscriptions: Vec<String>,
    ) -> Result<()> {
        log::info!("Unsubscribing all active offers");

        for subscription in subscriptions.iter() {
            log::info!("Unsubscribing: {}", subscription);
            market_api.unsubscribe(&subscription).await?;
        }
        log::info!("All Offers unsubscribed successfully.");
        Ok(())
    }

    // =========================================== //
    // Public api for running single market step
    // =========================================== //

    pub async fn run_step(
        addr: Addr<ProviderMarket>,
        market_api: Arc<MarketProviderApi>,
        subscriptions: HashMap<String, OfferSubscription>,
    ) -> Result<()> {
        for (id, subscription) in subscriptions {
            match market_api.collect(&id, Some(2.0), Some(2)).await {
                Err(error) => log::error!("Can't query market events. Error: {}", error),
                Ok(events) => {
                    ProviderMarket::dispatch_events(
                        events,
                        addr.clone(),
                        market_api.clone(),
                        subscription,
                    )
                    .await
                }
            }
        }

        Ok(())
    }

    // =========================================== //
    // Market internals - events processing
    // =========================================== //

    async fn dispatch_events(
        events: Vec<ProviderEvent>,
        addr: Addr<ProviderMarket>,
        market_api: Arc<MarketProviderApi>,
        subscription: OfferSubscription,
    ) {
        if events.len() == 0 {
            return;
        };

        log::info!(
            "Collected {} market events for subscription [{}]. Processing...",
            events.len(),
            &subscription.subscription_id
        );

        let dispatch_futures = events
            .iter()
            .map(|event| {
                ProviderMarket::dispatch_event(
                    addr.clone(),
                    market_api.clone(),
                    subscription.clone(),
                    event,
                )
            })
            .collect::<Vec<_>>();

        let _ = join_all(dispatch_futures)
            .await
            .iter()
            .map(|result| {
                if let Err(error) = result {
                    log::error!(
                        "Error processing event: {}, subscription_id: {}.",
                        error,
                        subscription.subscription_id
                    );
                }
            })
            .collect::<Vec<_>>();
    }

    async fn dispatch_event(
        addr: Addr<ProviderMarket>,
        market_api: Arc<MarketProviderApi>,
        subscription: OfferSubscription,
        event: &ProviderEvent,
    ) -> Result<()> {
        match event {
            ProviderEvent::ProposalEvent { proposal, .. } => {
                ProviderMarket::process_proposal(addr, market_api, subscription, proposal).await
            }
            ProviderEvent::AgreementEvent { agreement, .. } => {
                ProviderMarket::process_agreement(addr, market_api, subscription, agreement).await
            }
            _ => unimplemented!(),
        }
    }

    async fn process_proposal(
        addr: Addr<ProviderMarket>,
        market_api: Arc<MarketProviderApi>,
        subscription: OfferSubscription,
        demand: &Proposal,
    ) -> Result<()> {
        let proposal_id = demand.proposal_id()?;
        let subscription_id = subscription.subscription_id.clone();
        let offer = subscription.offer.clone();

        match addr
            .send(GotProposal::new(subscription, demand.clone()))
            .await?
        {
            Ok(action) => match action {
                ProposalResponse::CounterProposal { offer } => {
                    market_api
                        .counter_proposal(&offer, &subscription_id)
                        .await?;
                }
                ProposalResponse::AcceptProposal => {
                    let offer = demand.counter_offer(offer)?;
                    market_api
                        .counter_proposal(&offer, &subscription_id)
                        .await?;
                }
                ProposalResponse::IgnoreProposal => {
                    log::info!("Ignoring proposal {:?}", proposal_id)
                }
                ProposalResponse::RejectProposal => {
                    market_api
                        .reject_proposal(&subscription_id, proposal_id)
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
        addr: Addr<ProviderMarket>,
        market_api: Arc<MarketProviderApi>,
        subscription: OfferSubscription,
        agreement: &Agreement,
    ) -> Result<()> {
        let response = addr
            .send(GotAgreement::new(subscription, agreement.clone()))
            .await?;
        match response {
            Ok(action) => match action {
                AgreementResponse::ApproveAgreement => {
                    market_api
                        .approve_agreement(&agreement.agreement_id, Some(10.0))
                        .await?;

                    // We negotiated agreement and here responsibility of ProviderMarket ends.
                    // Notify outside world about agreement for further processing.
                    let message = AgreementApproved {
                        agreement: agreement.clone(),
                    };

                    let _ = addr.send(message).await?;
                }
                AgreementResponse::RejectAgreement => {
                    market_api.reject_agreement(&agreement.agreement_id).await?;
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

    // =========================================== //
    // Market internals - proposals and agreements reactions
    // =========================================== //

    #[logfn_inputs(Debug, fmt = "{}Processing {:?} {:?}")]
    #[logfn(Debug, fmt = "decided to: {:?}")]
    fn on_proposal(&mut self, msg: GotProposal, _ctx: &mut Context<Self>) -> Result<ProposalResponse> {
        self.negotiator
            .react_to_proposal(&msg.subscription.offer, &msg.proposal)
    }

    #[logfn_inputs(Debug, fmt = "{}Processing {:?} {:?}")]
    #[logfn(Debug, fmt = "decided to: {:?}")]
    fn on_agreement(&mut self, msg: GotAgreement, _ctx: &mut Context<Self>) -> Result<AgreementResponse> {
        self.negotiator.react_to_agreement(&msg.agreement)
    }

    #[logfn_inputs(Debug, fmt = "{}Got {:?} {:?}")]
    fn on_agreement_approved(&mut self, msg: AgreementApproved, _ctx: &mut Context<Self>) -> Result<()> {
        // At this moment we only forward agreement to outside world.
        self.agreement_signed_signal.send_signal(AgreementApproved {
            agreement: msg.agreement,
        })
    }

    // =========================================== //
    // Market internals - event subscription
    // =========================================== //

    pub fn on_subscribe(&mut self, msg: Subscribe<AgreementApproved>, _ctx: &mut Context<Self>) -> Result<()> {
        self.agreement_signed_signal.on_subscribe(msg);
        Ok(())
    }

    pub fn list_subscriptions(&self) -> Vec<String> {
        self.offer_subscriptions
            .keys()
            .map(|id| id.clone())
            .collect()
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
        let client = self.market_api.clone();
        let address = ctx.address();

        let fut = ProviderMarket::run_step(address, client, self.offer_subscriptions.clone());
        ActorResponse::r#async(fut.into_actor(self))
    }
}

impl Handler<CreateOffer> for ProviderMarket {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: CreateOffer, ctx: &mut Context<Self>) -> Self::Result {
        log::info!("Creating initial offer.");

        match self.negotiator.create_offer(&msg.offer_definition) {
            Ok(offer) => {
                let addr = ctx.address();
                let client = self.market_api.clone();

                log::info!("Subscribing to events...");

                ActorResponse::r#async(
                    async move {
                        ProviderMarket::create_offer(addr, client, offer)
                            .await
                            .map_err(|error| {
                                log::error!("Can't subscribe new offer, error: {}", error);
                                error
                            })
                    }
                    .into_actor(self),
                )
            }
            Err(error) => {
                log::error!("Negotiator failed to create offer. Error: {}", error);
                ActorResponse::reply(Err(error))
            }
        }
    }
}

impl Handler<OnShutdown> for ProviderMarket {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, _msg: OnShutdown, _ctx: &mut Context<Self>) -> Self::Result {
        let subscriptions = self.list_subscriptions();
        let client = self.market_api.clone();

        ActorResponse::r#async(ProviderMarket::on_shutdown(client, subscriptions).into_actor(self))
    }
}

forward_actix_handler!(ProviderMarket, GotProposal, on_proposal);
forward_actix_handler!(ProviderMarket, GotAgreement, on_agreement);
forward_actix_handler!(ProviderMarket, OfferSubscription, on_offer_subscribed);
forward_actix_handler!(ProviderMarket, Subscribe<AgreementApproved>, on_subscribe);
forward_actix_handler!(ProviderMarket, AgreementApproved, on_agreement_approved);

// =========================================== //
// Negotiators factory
// =========================================== //

fn create_negotiator(name: &str) -> Box<dyn Negotiator> {
    match name {
        "AcceptAll" => Box::new(AcceptAllNegotiator::new()),
        _ => {
            log::warn!("Unknown negotiator type {}. Using default: AcceptAll", name);
            Box::new(AcceptAllNegotiator::new())
        }
    }
}
