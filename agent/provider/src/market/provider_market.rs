use super::mock_negotiator::AcceptAllNegotiator;
use super::negotiator::{AgreementResponse, Negotiator, ProposalResponse};
use crate::utils::actix_signal::{SignalSlot, Subscribe};
use crate::{gen_actix_handler_async, gen_actix_handler_sync};

use ya_client::market::{ApiClient, ProviderApi};
use ya_model::market::{AgreementProposal, Offer, Proposal, ProviderEvent};

use actix::prelude::*;
use anyhow::{Error, Result};
use futures::future::join_all;
use log::{error, info, warn};
use std::cell::RefCell;
use std::rc::Rc;

// Temporrary
use chrono::{Duration, Utc};
use serde_json;
use ya_agent_offer_model::OfferDefinition;
use ya_client::web::WebClient;

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
    offer: OfferDefinition,
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
    market_api: ProviderApi,
    offers: Vec<OfferSubscription>,

    /// External actors can listen on this signal.
    pub agreement_signed_signal: SignalSlot<AgreementSigned>,
}

impl ProviderMarket {
    // =========================================== //
    // Initialization
    // =========================================== //

    pub fn new(market_api: ProviderApi, negotiator_type: &str) -> ProviderMarket {
        let negotiator = create_negotiator(negotiator_type);
        return ProviderMarket {
            market_api,
            negotiator,
            offers: vec![],
            agreement_signed_signal: SignalSlot::<AgreementSigned>::new(),
        };
    }

    pub async fn create_offer(&mut self, msg: CreateOffer) -> Result<()> {
        info!("Creating initial offer.");

        let offer = self.negotiator.create_offer(&msg.offer)?;

        info!("Subscribing to events.");

        let subscription_id = self.market_api.subscribe_offer(&offer).await?;
        self.offers.push(OfferSubscription {
            subscription_id,
            offer,
        });
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn onshutdown(&mut self, _msg: OnShutdown) -> Result<()> {
        info!("Unsubscribing events.");

        for offer in self.offers.iter() {
            self.market_api
                .unsubscribe_offer(&offer.subscription_id)
                .await?;
        }
        Ok(())
    }

    // =========================================== //
    // Public api for running single market step
    // =========================================== //

    pub async fn run_step(&self, _msg: UpdateMarket) -> Result<()> {
        for offer in self.offers.iter() {
            let events = self.query_events(&offer.subscription_id).await?;
            self.dispatch_events(&offer.subscription_id, &events).await;
        }

        Ok(())
    }

    // =========================================== //
    // Market internals - events processing
    // =========================================== //

    async fn query_events(&self, subscription_id: &str) -> Result<Vec<ProviderEvent>> {
        Ok(self
            .market_api
            .collect_demands(subscription_id, Some(1), Some(2))
            .await?)
    }

    async fn dispatch_events(&self, subscription_id: &str, events: &Vec<ProviderEvent>) {
        info!("Collected {} market events. Processing...", events.len());

        let dispatch_futures = events
            .iter()
            .map(|event| self.dispatch_event(subscription_id, event))
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

    async fn dispatch_event(&self, subscription_id: &str, event: &ProviderEvent) -> Result<()> {
        match event {
            ProviderEvent::ProposalEvent { proposal, .. } => {
                let proposal_id = proposal.proposal_id.as_ref().unwrap();

                info!("Got demand [id={}].", proposal_id);

                let agreement_proposal = self
                    .market_api
                    .get_proposal_demand(subscription_id, proposal_id)
                    .await?;

                self.process_proposal(subscription_id, agreement_proposal)
                    .await?;
            }
            ProviderEvent::AgreementEvent {
                agreement, /**demand,**/
                ..
            } => {
                let agreement_id = &agreement.agreement_id;
                info!("Got agreement [id={}].", agreement_id);

                // Temporary workaround. Update after new market api will aprear.
                //                let agreement_proposal = self.api.provider()
                //                    .get_proposal(subscription_id, demand.id)
                //                    .await?;

                //let offer = Proposal::new("".to_string(), serde_json::json!({}), "".to_string());
                //let demand = Proposal::new("".to_string(), serde_json::json!({}), "".to_string());
                let agreement_proposal =
                    AgreementProposal::new("".to_string(), Utc::now() + Duration::weeks(2));

                self.process_agreement(subscription_id, agreement_proposal, &agreement_id)
                    .await?;
            }
            _ => unimplemented!(),
        }
        Ok(())
    }

    async fn process_proposal(&self, subscription_id: &str, proposal: Proposal) -> Result<()> {
        let response = self.negotiator.react_to_proposal(&proposal);
        match response {
            Ok(action) => match action {
                ProposalResponse::AcceptProposal => {
                    self.accept_proposal(subscription_id, &proposal).await?
                }
                ProposalResponse::CounterProposal { proposal } => {
                    self.counter_proposal(subscription_id, proposal).await?
                }
                ProposalResponse::IgnoreProposal => info!("Ignoring proposal {:?}.", proposal),
                ProposalResponse::RejectProposal => {
                    self.reject_proposal(subscription_id, &proposal).await?
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
        &self,
        subscription_id: &str,
        agreement: AgreementProposal,
        agreement_id: &str,
    ) -> Result<()> {
        let response = self.negotiator.react_to_agreement(&agreement);
        match response {
            Ok(action) => match action {
                AgreementResponse::ApproveAgreement => {
                    self.approve_agreement(subscription_id, agreement_id)
                        .await?
                }
                AgreementResponse::RejectAgreement => {
                    self.reject_agreement(subscription_id, agreement_id).await?
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

    async fn accept_proposal(&self, subscription_id: &str, proposal: &Proposal) -> Result<()> {
        info!(
            "Accepting proposal [{:?}] without changes, subscription_id: {}.",
            proposal.proposal_id, subscription_id
        );

        // Note: Provider can't create agreement - only requestor can. We can accept
        // proposal, by resending the same offer as we got from requestor.
        self.market_api
            .create_proposal_offer(
                &proposal,
                subscription_id,
                proposal.proposal_id.as_ref().unwrap(),
            )
            .await?;
        Ok(())
    }

    async fn counter_proposal(&self, subscription_id: &str, proposal: Proposal) -> Result<()> {
        info!(
            "Sending counter offer to proposal [{:?}], subscription_id: {}.",
            proposal.proposal_id, subscription_id
        );

        self.market_api
            .create_proposal_offer(
                &proposal,
                subscription_id,
                proposal.proposal_id.as_ref().unwrap(),
            )
            .await?;
        Ok(())
    }

    async fn reject_proposal(&self, subscription_id: &str, proposal: &Proposal) -> Result<()> {
        info!(
            "Rejecting proposal [{:?}], subscription_id: {}.",
            proposal.proposal_id, subscription_id
        );

        self.market_api
            .reject_proposal_demand(subscription_id, proposal.proposal_id.as_ref().unwrap())
            .await?;
        Ok(())
    }

    async fn approve_agreement(&self, subscription_id: &str, agreement_id: &str) -> Result<()> {
        info!(
            "Accepting agreement [{}], subscription_id: {}.",
            agreement_id, subscription_id
        );

        self.market_api.approve_agreement(agreement_id).await?;

        // We negotiated agreement and here responsibility of ProviderMarket ends.
        // Notify outside world about agreement for further processing.
        let message = AgreementSigned {
            agreement_id: agreement_id.to_string(),
        };
        self.agreement_signed_signal.send_signal(message)?;
        Ok(())
    }

    async fn reject_agreement(&self, subscription_id: &str, agreement_id: &str) -> Result<()> {
        info!(
            "Rejecting agreement [{}], subscription_id: {}.",
            agreement_id, subscription_id
        );

        self.market_api.reject_agreement(agreement_id).await?;
        Ok(())
    }

    // =========================================== //
    // Market internals - event subscription
    // =========================================== //

    pub fn on_subscribe(&mut self, msg: Subscribe<AgreementSigned>) -> Result<()> {
        self.agreement_signed_signal.on_subscribe(msg);
        Ok(())
    }
}

// =========================================== //
// Helper functions
// =========================================== //

impl CreateOffer {
    pub fn new(offer: OfferDefinition) -> CreateOffer {
        CreateOffer { offer }
    }
}

// =========================================== //
// Actix stuff
// =========================================== //

/// Wrapper for ProviderMarket. It is neccesary to use self in async futures.
pub struct ProviderMarketActor {
    market: Rc<RefCell<ProviderMarket>>,
}

impl ProviderMarketActor {
    pub fn new(api: ProviderApi, negotiator_type: &str) -> ProviderMarketActor {
        let rc = Rc::new(RefCell::new(ProviderMarket::new(api, negotiator_type)));
        ProviderMarketActor { market: rc }
    }
}

impl Actor for ProviderMarketActor {
    type Context = Context<Self>;
}

gen_actix_handler_async!(ProviderMarketActor, CreateOffer, create_offer, market);
gen_actix_handler_async!(ProviderMarketActor, UpdateMarket, run_step, market);
gen_actix_handler_async!(ProviderMarketActor, OnShutdown, onshutdown, market);
gen_actix_handler_sync!(
    ProviderMarketActor,
    Subscribe<AgreementSigned>,
    on_subscribe,
    market
);

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
