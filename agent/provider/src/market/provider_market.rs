use super::mock_negotiator::AcceptAllNegotiator;
use super::negotiator::{AgreementResponse, Negotiator, ProposalResponse};
use crate::utils::actix_signal::{SignalSlot, Subscribe};
use crate::forward_actix_handler;
use crate::utils::actix_handler::ResultTypeGetter;

use ya_client::market::ApiClient;
use ya_model::market::{AgreementProposal, Offer, Proposal, ProviderEvent};

use actix::prelude::*;
use anyhow::{Error, Result};
use futures::future::join_all;
use log::{error, info, warn};
use std::sync::Arc;

// Temporrary
use serde_json;
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
    proposal: AgreementProposal
}

#[derive(Message)]
#[rtype(result = "Result<AgreementResponse>")]
#[allow(dead_code)]
pub struct GotAgreement {
    subscription_id: String,
    agreement: AgreementProposal,
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
    offer: OfferSubscription
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
    api: Arc<ApiClient>,
    offers: Vec<OfferSubscription>,

    /// External actors can listen on this signal.
    pub agreement_signed_signal: SignalSlot<AgreementSigned>,
}

impl ProviderMarket {
    // =========================================== //
    // Initialization
    // =========================================== //

    pub fn new(api: ApiClient, negotiator_type: &str) -> ProviderMarket {
        let negotiator = create_negotiator(negotiator_type);
        return ProviderMarket {
            api: Arc::new(api),
            negotiator,
            offers: vec![],
            agreement_signed_signal: SignalSlot::<AgreementSigned>::new(),
        };
    }

    pub async fn create_offer(addr: Addr<ProviderMarket>, client: Arc<ApiClient>, offer: Offer) -> Result<()> {
        let subscription_id = client.provider().subscribe(&offer).await?;
        let sub = OfferSubscription{subscription_id, offer};

        let _ = addr.send(OnOfferSubscribed{offer: sub}).await?;
        Ok(())
    }

    fn offer_subscribed(&mut self, msg: OnOfferSubscribed) -> Result<()> {
        let subscription_id = &msg.offer.subscription_id;
        info!("Subscribed to events for offer [{}].", subscription_id);

        Ok(self.offers.push(msg.offer ))
    }

    #[allow(dead_code)]
    pub async fn onshutdown(client: Arc<ApiClient>, subscriptions: Vec<String>) -> Result<()> {
        info!("Unsubscribing events");

        for subscription_id in subscriptions.iter() {
            client.provider()
                .unsubscribe(subscription_id)
                .await?;
        }
        info!("Unsubscribing events finished.");
        Ok(())
    }

    // =========================================== //
    // Public api for running single market step
    // =========================================== //

    pub async fn run_step(addr: Addr<ProviderMarket>, client: Arc<ApiClient>, subscriptions: Vec<String>) -> Result<()> {
        for subscription in subscriptions.iter() {
            let events = ProviderMarket::query_events(client.clone(), &subscription).await?;
            ProviderMarket::dispatch_events(addr.clone(), client.clone(), &subscription, &events).await;
        }

        Ok(())
    }

    // =========================================== //
    // Market internals - events processing
    // =========================================== //

    async fn query_events(client: Arc<ApiClient>, subscription_id: &str) -> Result<Vec<ProviderEvent>> {
        Ok(client.provider()
            .collect(subscription_id, Some(1), Some(2))
            .await?)
    }

    async fn dispatch_events(addr: Addr<ProviderMarket>, client: Arc<ApiClient>, subscription_id: &str, events: &Vec<ProviderEvent>) {
        info!("Collected {} market events. Processing...", events.len());

        let dispatch_futures = events
            .iter()
            .map(|event| ProviderMarket::dispatch_event(addr.clone(), client.clone(), subscription_id, event))
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
    }

    async fn dispatch_event(addr: Addr<ProviderMarket>, client: Arc<ApiClient>, subscription_id: &str, event: &ProviderEvent) -> Result<()> {
        match event {
            ProviderEvent::DemandEvent { demand, .. } => {
                let proposal_id = &demand.as_ref().ok_or(Error::msg("no proposal id"))?.id;

                info!("Got demand [id={}].", proposal_id);

                let agreement_proposal = client.provider()
                    .get_proposal(subscription_id, proposal_id)
                    .await?;

                ProviderMarket::process_proposal(addr, client, subscription_id, agreement_proposal)
                    .await?;
            }
            ProviderEvent::NewAgreementEvent {
                agreement_id, /**demand,**/
                ..
            } => {
                let agreement_id = &agreement_id.as_ref().ok_or(Error::msg("no agreement id"))?;
                info!("Got agreement [id={}].", agreement_id);

                // Temporary workaround. Update after new market api will aprear.
                //                let agreement_proposal = self.api.provider()
                //                    .get_proposal(subscription_id, demand.id)
                //                    .await?;

                let offer = Proposal::new("".to_string(), serde_json::json!({}), "".to_string());
                let demand = Proposal::new("".to_string(), serde_json::json!({}), "".to_string());
                let agreement_proposal = AgreementProposal::new("".to_string(), demand, offer);

                ProviderMarket::process_agreement(addr, client, subscription_id, agreement_proposal, &agreement_id)
                    .await?;
            }
        }
        Ok(())
    }

    async fn process_proposal(
        addr: Addr<ProviderMarket>,
        client: Arc<ApiClient>,
        subscription_id: &str,
        proposal: AgreementProposal,
    ) -> Result<()> {
        let response = addr.send(GotProposal::new(subscription_id, &proposal)).await?;
        match response {
            Ok(action) => match action {
                ProposalResponse::AcceptProposal => {
                    ProviderMarket::accept_proposal(client, subscription_id, &proposal).await?
                }
                ProposalResponse::CounterProposal { proposal } => {
                    ProviderMarket::counter_proposal(client, subscription_id, proposal).await?
                }
                ProposalResponse::IgnoreProposal => info!("Ignoring proposal {}.", proposal.id),
                ProposalResponse::RejectProposal => {
                    ProviderMarket::reject_proposal(client, subscription_id, &proposal).await?
                }
            },
            Err(error) => error!(
                "Negotiator error while processing proposal {}. Error: {}",
                proposal.id, error
            ),
        }
        Ok(())
    }

    async fn process_agreement(
        addr: Addr<ProviderMarket>,
        client: Arc<ApiClient>,
        subscription_id: &str,
        agreement: AgreementProposal,
        agreement_id: &str,
    ) -> Result<()> {
        let response = addr.send(GotAgreement::new(subscription_id, &agreement)).await?;
        match response {
            Ok(action) => match action {
                AgreementResponse::ApproveAgreement => {
                    ProviderMarket::approve_agreement(addr, client, subscription_id, agreement_id).await?
                }
                AgreementResponse::RejectAgreement => {
                    ProviderMarket::reject_agreement(client, subscription_id, agreement_id).await?
                }
            },
            Err(error) => error!(
                "Negotiator error while processing agreement {}. Error: {}",
                agreement_id, error
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
        self.agreement_signed_signal.send_signal(AgreementSigned{agreement_id: msg.agreement_id})
    }

    async fn accept_proposal(
        client: Arc<ApiClient>,
        subscription_id: &str,
        proposal: &AgreementProposal,
    ) -> Result<()> {
        info!(
            "Accepting proposal [{}] without changes, subscription_id: {}.",
            proposal.id, subscription_id
        );

        // Note: Provider can't create agreement - only requestor can. We can accept
        // proposal, by resending the same offer as we got from requestor.
        client.provider()
            .create_proposal(&proposal.offer, subscription_id, &proposal.id)
            .await?;
        Ok(())
    }

    async fn counter_proposal(client: Arc<ApiClient>, subscription_id: &str, proposal: Proposal) -> Result<()> {
        info!(
            "Sending counter offer to proposal [{}], subscription_id: {}.",
            proposal.id, subscription_id
        );

        client.provider()
            .create_proposal(&proposal, subscription_id, &proposal.id)
            .await?;
        Ok(())
    }

    async fn reject_proposal(
        client: Arc<ApiClient>,
        subscription_id: &str,
        proposal: &AgreementProposal,
    ) -> Result<()> {
        info!(
            "Rejecting proposal [{}], subscription_id: {}.",
            proposal.id, subscription_id
        );

        client.provider()
            .reject_proposal(subscription_id, &proposal.id)
            .await?;
        Ok(())
    }

    async fn approve_agreement(addr: Addr<ProviderMarket>, client: Arc<ApiClient>, subscription_id: &str, agreement_id: &str) -> Result<()> {
        info!(
            "Accepting agreement [{}], subscription_id: {}.",
            agreement_id, subscription_id
        );

        client.provider().approve_agreement(agreement_id).await?;

        // We negotiated agreement and here responsibility of ProviderMarket ends.
        // Notify outside world about agreement for further processing.
        let message = OnAgreementSigned {
            agreement_id: agreement_id.to_string(),
        };

        let _ = addr.send(message).await?;
        Ok(())
    }

    async fn reject_agreement(client: Arc<ApiClient>, subscription_id: &str, agreement_id: &str) -> Result<()> {
        info!(
            "Rejecting agreement [{}], subscription_id: {}.",
            agreement_id, subscription_id
        );

        client.provider().reject_agreement(agreement_id).await?;
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
        self.offers.iter()
            .map(|offer|{
                offer.subscription_id.clone()
            }).collect()
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
        let client = self.api.clone();
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
                let client = self.api.clone();

                info!("Subscribing to events...");

                ActorResponse::r#async(
                async move {
                        let result = ProviderMarket::create_offer(addr, client, offer).await;
                        if let Err(error) = result {
                            error!("Can't subscribe new offer, error: {}", error);
                        }
                        Ok(())
                    }
                    .into_actor(self))
            },
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
        let client = self.api.clone();

        ActorResponse::r#async(ProviderMarket::onshutdown(client, subscriptions).into_actor(self))
    }
}


forward_actix_handler!(ProviderMarket, GotProposal, on_proposal);
forward_actix_handler!(ProviderMarket, GotAgreement, on_agreement);
forward_actix_handler!(ProviderMarket, OnOfferSubscribed, offer_subscribed);
forward_actix_handler!(
    ProviderMarket,
    Subscribe<AgreementSigned>,
    on_subscribe
);
forward_actix_handler!(ProviderMarket, OnAgreementSigned, on_agreement_signed);


// =========================================== //
// Messages creation
// =========================================== //

impl CreateOffer {
    pub fn new(offer: OfferDefinition) -> CreateOffer {
        CreateOffer { offer_definition: offer }
    }
}

impl GotProposal {
    pub fn new(subscription_id: &str, proposal: &AgreementProposal) -> GotProposal {
        GotProposal{subscription_id: subscription_id.to_string(), proposal: proposal.clone()}
    }
}

impl GotAgreement {
    pub fn new(subscription_id: &str, proposal: &AgreementProposal) -> GotAgreement {
        GotAgreement{subscription_id: subscription_id.to_string(), agreement: proposal.clone()}
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
