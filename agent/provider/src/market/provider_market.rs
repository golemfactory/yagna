use actix::prelude::*;
use anyhow::{Error, Result};
use futures::future::join_all;
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

/// This event is emmited, when agreement is already signed
/// and provider can go to activity stage and task creation.
/// TODO: We should pass whole agreement here with negotiated offers.
#[derive(Message, Clone)]
#[rtype(result = "Result<()>")]
pub struct AgreementSigned {
    pub agreement_id: String,
}

/// Sends offer to market.
#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct CreateOffer {
    offer_definition: OfferDefinition,
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

#[derive(Message)]
#[rtype(result = "Result<ProposalResponse>")]
#[allow(dead_code)]
pub struct GotProposal {
    subscription_id: String,
    proposal: Proposal,
}

#[derive(Message)]
#[rtype(result = "Result<AgreementResponse>")]
#[allow(dead_code)]
pub struct GotAgreement {
    subscription_id: String,
    agreement: Agreement,
}

/// Async code emmits this event to ProviderMarket, which reacts to it
/// and broadcasts AgreementSigned event to external world.
#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct OnAgreementSigned {
    pub agreement_id: String,
}

/// Send when subscribing to market will be finished.
#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct OnOfferSubscribed {
    offer_subscription: OfferSubscription,
}

// =========================================== //
// ProviderMarket declaration
// =========================================== //

struct OfferSubscription {
    subscription_id: String,
    offer: Offer,
}

/// Manages market api communication and forwards proposal to implementation of market strategy.
pub struct ProviderMarket {
    negotiator: Box<dyn Negotiator>,
    market_api: Arc<MarketProviderApi>,
    offer_subscriptions: HashMap<String, OfferSubscription>,

    /// External actors can listen on this signal.
    pub agreement_signed_signal: SignalSlot<AgreementSigned>,
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
            agreement_signed_signal: SignalSlot::<AgreementSigned>::new(),
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

        let _ = addr
            .send(OnOfferSubscribed {
                offer_subscription: sub,
            })
            .await?;
        Ok(())
    }

    fn offer_subscribed(&mut self, msg: OnOfferSubscribed) -> Result<()> {
        let subscription_id = &msg.offer_subscription.subscription_id;
        log::info!(
            "Subscribed offer: {:?}\nsubscription_id: {}",
            msg.offer_subscription.offer,
            subscription_id
        );
        self.offer_subscriptions
            .insert(subscription_id.clone(), msg.offer_subscription);
        Ok(())
    }

    async fn on_shutdown(
        market_api: Arc<MarketProviderApi>,
        subscriptions: Vec<String>,
    ) -> Result<()> {
        log::info!("Unsubscribing all active offers");

        for subscription_id in subscriptions.iter() {
            log::info!("Unsubscribing: {}", subscription_id);
            market_api.unsubscribe(subscription_id).await?;
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
        subscriptions: Vec<String>,
    ) -> Result<()> {
        for subscription in subscriptions.iter() {
            match market_api.collect(subscription, Some(2.0), Some(2)).await {
                Err(error) => log::error!("Can't query market events. Error: {}", error),
                Ok(events) => {
                    ProviderMarket::dispatch_events(
                        events,
                        addr.clone(),
                        market_api.clone(),
                        &subscription,
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
        subscription_id: &str,
    ) {
        log::info!("Collected {} market events. Processing...", events.len());

        let dispatch_futures = events
            .iter()
            .map(|event| {
                ProviderMarket::dispatch_event(
                    addr.clone(),
                    market_api.clone(),
                    subscription_id,
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
                        subscription_id
                    );
                }
            })
            .collect::<Vec<_>>();
    }

    async fn dispatch_event(
        addr: Addr<ProviderMarket>,
        market_api: Arc<MarketProviderApi>,
        subscription_id: &str,
        event: &ProviderEvent,
    ) -> Result<()> {
        match event {
            ProviderEvent::ProposalEvent { proposal, .. } => {
                ProviderMarket::process_proposal(addr, market_api, subscription_id, proposal).await
            }
            ProviderEvent::AgreementEvent { agreement, .. } => {
                ProviderMarket::process_agreement(addr, market_api, subscription_id, agreement)
                    .await
            }
            _ => unimplemented!(),
        }
    }

    async fn process_proposal(
        addr: Addr<ProviderMarket>,
        market_api: Arc<MarketProviderApi>,
        subscription_id: &str,
        demand: &Proposal,
    ) -> Result<()> {
        let proposal_id = demand.proposal_id()?;
        log::info!("Got demand proposal [{:#?}]. Processing...", demand);

        match addr.send(GotProposal::new(subscription_id, demand)).await? {
            Ok(action) => match action {
                ProposalResponse::CounterProposal { offer } => {
                    market_api.counter_proposal(&offer, subscription_id).await?;
                }
                ProposalResponse::IgnoreProposal => {
                    log::info!("Ignoring proposal {:?}.", proposal_id)
                }
                ProposalResponse::RejectProposal => {
                    market_api
                        .reject_proposal(subscription_id, proposal_id)
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
        subscription_id: &str,
        agreement: &Agreement,
    ) -> Result<()> {
        log::info!(
            "Got agreement [id={}]. Processing...",
            agreement.agreement_id
        );

        let response = addr
            .send(GotAgreement::new(subscription_id, agreement))
            .await?;
        match response {
            Ok(action) => match action {
                AgreementResponse::ApproveAgreement => {
                    market_api
                        .approve_agreement(&agreement.agreement_id, Some(10.0))
                        .await?;

                    // We negotiated agreement and here responsibility of ProviderMarket ends.
                    // Notify outside world about agreement for further processing.
                    let message = OnAgreementSigned {
                        agreement_id: agreement.agreement_id.to_string(),
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

    fn on_proposal(&mut self, msg: GotProposal) -> Result<ProposalResponse> {
        let offer = match self.offer_subscriptions.get(&msg.subscription_id) {
            Some(offer_subscription) => &offer_subscription.offer,
            None => anyhow::bail!("No such subscription: {}", msg.subscription_id),
        };
        self.negotiator.react_to_proposal(offer, &msg.proposal)
    }

    fn on_agreement(&mut self, msg: GotAgreement) -> Result<AgreementResponse> {
        self.negotiator.react_to_agreement(&msg.agreement)
    }

    fn on_agreement_signed(&mut self, msg: OnAgreementSigned) -> Result<()> {
        // At this moment we only forward agreement to outside world.
        self.agreement_signed_signal.send_signal(AgreementSigned {
            agreement_id: msg.agreement_id,
        })
    }

    // =========================================== //
    // Market internals - event subscription
    // =========================================== //

    pub fn on_subscribe(&mut self, msg: Subscribe<AgreementSigned>) -> Result<()> {
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
        let subscriptions = self.list_subscriptions();
        let client = self.market_api.clone();
        let address = ctx.address();

        ActorResponse::r#async(
            async move { ProviderMarket::run_step(address, client, subscriptions).await }
                .into_actor(self),
        )
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
forward_actix_handler!(ProviderMarket, OnOfferSubscribed, offer_subscribed);
forward_actix_handler!(ProviderMarket, Subscribe<AgreementSigned>, on_subscribe);
forward_actix_handler!(ProviderMarket, OnAgreementSigned, on_agreement_signed);

// =========================================== //
// Messages creation
// =========================================== //

impl CreateOffer {
    pub fn new(offer: OfferDefinition) -> CreateOffer {
        CreateOffer {
            offer_definition: offer,
        }
    }
}

impl GotProposal {
    pub fn new(subscription_id: &str, proposal: &Proposal) -> GotProposal {
        GotProposal {
            subscription_id: subscription_id.to_string(),
            proposal: proposal.clone(),
        }
    }
}

impl GotAgreement {
    pub fn new(subscription_id: &str, proposal: &Agreement) -> GotAgreement {
        GotAgreement {
            subscription_id: subscription_id.to_string(),
            agreement: proposal.clone(),
        }
    }
}

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
