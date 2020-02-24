use super::mock_negotiator::AcceptAllNegotiator;
use super::negotiator::{AgreementResponse, Negotiator, ProposalResponse};

use ya_client::market::MarketProviderApi;
use ya_model::market::{Agreement, Offer, Proposal, ProviderEvent};
use ya_utils_actix::actix_handler::ResultTypeGetter;
use ya_utils_actix::actix_signal::{SignalSlot, Subscribe};
use ya_utils_actix::forward_actix_handler;

use actix::prelude::*;
use anyhow::{Error, Result};
use futures::future::join_all;
use log::{error, info, warn};
use std::sync::Arc;

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
    offer: OfferSubscription,
}

// =========================================== //
// ProviderMarket declaration
// =========================================== //

#[allow(dead_code)]
struct OfferSubscription {
    subscription_id: String,
    offer: Offer,
}

/// Manages market api communication and forwards proposal to implementation of market strategy.
pub struct ProviderMarket {
    negotiator: Box<dyn Negotiator>,
    market_api: Arc<MarketProviderApi>,
    offers: Vec<OfferSubscription>,

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
            offers: vec![],
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

        let _ = addr.send(OnOfferSubscribed { offer: sub }).await?;
        Ok(())
    }

    fn offer_subscribed(&mut self, msg: OnOfferSubscribed) -> Result<()> {
        let subscription_id = &msg.offer.subscription_id;
        info!("Subscribed to events for offer [{}].", subscription_id);

        Ok(self.offers.push(msg.offer))
    }

    async fn onshutdown(
        market_api: Arc<MarketProviderApi>,
        subscriptions: Vec<String>,
    ) -> Result<()> {
        info!("Unsubscribing events");

        for subscription_id in subscriptions.iter() {
            market_api.unsubscribe(subscription_id).await?;
        }
        info!("Unsubscribing events finished.");
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
            let _ =
                ProviderMarket::dispatch_events(addr.clone(), market_api.clone(), &subscription)
                    .await;
        }

        Ok(())
    }

    // =========================================== //
    // Market internals - events processing
    // =========================================== //

    async fn dispatch_events(
        addr: Addr<ProviderMarket>,
        market_api: Arc<MarketProviderApi>,
        subscription_id: &str,
    ) -> Result<()> {
        let events = market_api
            .collect(subscription_id, Some(1), Some(2))
            .await?;

        info!("Collected {} market events. Processing...", events.len());

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
                    error!(
                        "Error processing event: {}, subscription_id: {}.",
                        error, subscription_id
                    );
                }
            })
            .collect::<Vec<_>>();

        Ok(())
    }

    async fn dispatch_event(
        addr: Addr<ProviderMarket>,
        market_api: Arc<MarketProviderApi>,
        subscription_id: &str,
        event: &ProviderEvent,
    ) -> Result<()> {
        match event {
            ProviderEvent::ProposalEvent { proposal, .. } => {
                let proposal_id = &proposal.id().map_err(Error::msg)?;
                info!("Got demand proposal [id={}].", proposal_id);

                ProviderMarket::process_proposal(addr, market_api, subscription_id, proposal)
                    .await?;
            }
            ProviderEvent::AgreementEvent { agreement, .. } => {
                info!("Got agreement [id={}].", agreement.agreement_id);

                ProviderMarket::process_agreement(addr, market_api, subscription_id, agreement)
                    .await?;
            }
            _ => unimplemented!(),
        }
        Ok(())
    }

    async fn process_proposal(
        addr: Addr<ProviderMarket>,
        market_api: Arc<MarketProviderApi>,
        subscription_id: &str,
        proposal: &Proposal,
    ) -> Result<()> {
        let response = addr
            .send(GotProposal::new(subscription_id, proposal))
            .await?;
        match response {
            Ok(action) => match action {
                ProposalResponse::AcceptProposal => {
                    ProviderMarket::accept_proposal(market_api, subscription_id, proposal).await?
                }
                ProposalResponse::CounterProposal {
                    proposal: counter_proposal,
                } => {
                    ProviderMarket::counter_proposal(market_api, subscription_id, &counter_proposal)
                        .await?
                }
                ProposalResponse::IgnoreProposal => {
                    info!("Ignoring proposal {:?}.", proposal.proposal_id)
                }
                ProposalResponse::RejectProposal => {
                    ProviderMarket::reject_proposal(market_api, subscription_id, proposal).await?
                }
            },
            Err(error) => error!(
                "Negotiator error while processing proposal {:?}. Error: {}",
                proposal.proposal_id, error
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
        let response = addr
            .send(GotAgreement::new(subscription_id, agreement))
            .await?;
        match response {
            Ok(action) => match action {
                AgreementResponse::ApproveAgreement => {
                    ProviderMarket::approve_agreement(addr, market_api, subscription_id, agreement)
                        .await?
                }
                AgreementResponse::RejectAgreement => {
                    ProviderMarket::reject_agreement(addr, market_api, subscription_id, agreement)
                        .await?
                }
            },
            Err(error) => error!(
                "Negotiator error while processing agreement {}. Error: {}",
                agreement.agreement_id, error
            ),
        }
        Ok(())
    }

    // =========================================== //
    // Market internals - proposals and agreements reactions
    // =========================================== //

    fn on_proposal(&mut self, msg: GotProposal) -> Result<ProposalResponse> {
        self.negotiator.react_to_proposal(&msg.proposal)
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

    async fn accept_proposal(
        market_api: Arc<MarketProviderApi>,
        subscription_id: &str,
        proposal: &Proposal,
    ) -> Result<()> {
        info!(
            "Accepting proposal [{:?}] without changes, subscription_id: {}.",
            proposal.proposal_id, subscription_id
        );

        // Note: Provider can't create agreement - only requestor can. We can accept
        // proposal, by resending the same offer as we got from requestor.
        market_api
            .counter_proposal(
                proposal,
                subscription_id,
                proposal.id().map_err(Error::msg)?,
            )
            .await?;
        Ok(())
    }

    async fn counter_proposal(
        market_api: Arc<MarketProviderApi>,
        subscription_id: &str,
        proposal: &Proposal,
    ) -> Result<()> {
        info!(
            "Sending counter offer to proposal [{:?}], subscription_id: {}.",
            proposal.proposal_id, subscription_id
        );

        market_api
            .counter_proposal(
                proposal,
                subscription_id,
                proposal.id().map_err(Error::msg)?,
            )
            .await?;
        Ok(())
    }

    async fn reject_proposal(
        market_api: Arc<MarketProviderApi>,
        subscription_id: &str,
        proposal: &Proposal,
    ) -> Result<()> {
        info!(
            "Rejecting proposal [{:?}], subscription_id: {}.",
            proposal.proposal_id, subscription_id
        );

        market_api
            .reject_proposal(subscription_id, &proposal.id().map_err(Error::msg)?)
            .await?;
        Ok(())
    }

    async fn approve_agreement(
        addr: Addr<ProviderMarket>,
        market_api: Arc<MarketProviderApi>,
        subscription_id: &str,
        agreement: &Agreement,
    ) -> Result<()> {
        info!(
            "Accepting agreement [{}], subscription_id: {}.",
            agreement.agreement_id, subscription_id
        );

        market_api
            .approve_agreement(&agreement.agreement_id)
            .await?;

        // We negotiated agreement and here responsibility of ProviderMarket ends.
        // Notify outside world about agreement for further processing.
        let message = OnAgreementSigned {
            agreement_id: agreement.agreement_id.to_string(),
        };

        let _ = addr.send(message).await?;
        Ok(())
    }

    async fn reject_agreement(
        _addr: Addr<ProviderMarket>,
        market_api: Arc<MarketProviderApi>,
        subscription_id: &str,
        agreement: &Agreement,
    ) -> Result<()> {
        info!(
            "Rejecting agreement [{}], subscription_id: {}.",
            agreement.agreement_id, subscription_id
        );

        market_api.reject_agreement(&agreement.agreement_id).await?;
        Ok(())
    }

    // =========================================== //
    // Market internals - event subscription
    // =========================================== //

    pub fn on_subscribe(&mut self, msg: Subscribe<AgreementSigned>) -> Result<()> {
        self.agreement_signed_signal.on_subscribe(msg);
        Ok(())
    }

    pub fn list_subscriptions(&self) -> Vec<String> {
        self.offers
            .iter()
            .map(|offer| offer.subscription_id.clone())
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
        info!("Creating initial offer.");

        match self.negotiator.create_offer(&msg.offer_definition) {
            Ok(offer) => {
                let addr = ctx.address();
                let client = self.market_api.clone();

                info!("Subscribing to events...");

                ActorResponse::r#async(
                    async move {
                        let result = ProviderMarket::create_offer(addr, client, offer).await;
                        if let Err(error) = result {
                            error!("Can't subscribe new offer, error: {}", error);
                        }
                        Ok(())
                    }
                    .into_actor(self),
                )
            }
            Err(error) => {
                error!("Negotiator failed to create offer. Error: {}", error);
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

        ActorResponse::r#async(ProviderMarket::onshutdown(client, subscriptions).into_actor(self))
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
            warn!("Unknown negotiator type {}. Using default: AcceptAll", name);
            Box::new(AcceptAllNegotiator::new())
        }
    }
}
