use super::mock_negotiator::AcceptAllNegotiator;
use super::negotiator::{AgreementResponse, Negotiator, ProposalResponse};
use crate::node_info::NodeInfo;
use crate::utils::actix_signal::SignalSlot;

use ya_client::{market::ApiClient, Result};
use ya_model::market::{AgreementProposal, Offer, Proposal, ProviderEvent};

use futures::future::join_all;
use log::{error, info, warn};
use actix::prelude::*;

// Temporrary
use serde_json;

#[allow(dead_code)]
struct OfferSubscription {
    subscription_id: String,
    offer: Offer,
}

#[derive(Message, Clone)]
#[rtype(result="()")]
struct AgreementSigned {

}

/// Manages market api communication and forwards proposal to implementation of market strategy.
///
pub struct ProviderMarket {
    negotiator: Box<dyn Negotiator>,
    api: ApiClient,
    offers: Vec<OfferSubscription>,

    agreement_signed_signal: SignalSlot<AgreementSigned>,
}

impl ProviderMarket {
    // =========================================== //
    // Initialization
    // =========================================== //

    pub fn new(api: ApiClient, negotiator_type: &str) -> ProviderMarket {
        let negotiator = create_negotiator(negotiator_type);
        return ProviderMarket {
            api,
            negotiator,
            offers: vec![],
            agreement_signed_signal: SignalSlot::<AgreementSigned>::new()
        };
    }

    pub async fn create_offers(&mut self, node_info: &NodeInfo) -> Result<()> {
        info!("Creating initial offer.");

        let offer = self.negotiator.create_offer(node_info)?;

        info!("Subscribing to events.");

        let subscription_id = self.api.provider().subscribe(&offer).await?;
        self.offers.push(OfferSubscription {
            subscription_id,
            offer,
        });
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn onshutdown(&mut self) -> Result<()> {
        info!("Unsubscribing events.");

        for offer in self.offers.iter() {
            self.api
                .provider()
                .unsubscribe(&offer.subscription_id)
                .await?;
        }
        Ok(())
    }

    // =========================================== //
    // Public api for running single market step
    // =========================================== //

    pub async fn run_step(&self) -> Result<()> {
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
        self.api
            .provider()
            .collect(subscription_id, Some(1), Some(2))
            .await
    }

    async fn dispatch_events(&self, subscription_id: &str, events: &Vec<ProviderEvent>) {
        info!("Collected {} events. Processing...", events.len());

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
            ProviderEvent::DemandEvent { demand, .. } => {
                let proposal_id = &demand.as_ref().unwrap().id;

                info!("Got demand [id={}].", proposal_id);

                let agreement_proposal = self
                    .api
                    .provider()
                    .get_proposal(subscription_id, proposal_id)
                    .await?;

                self.process_proposal(subscription_id, agreement_proposal)
                    .await?;
            }
            ProviderEvent::NewAgreementEvent {
                agreement_id, /**demand,**/
                ..
            } => {
                let agreement_id = &agreement_id.as_ref().unwrap();
                //                let demand = demand.as_ref().unwrap();

                info!("Got agreement [id={}].", agreement_id);

                // Temporary workaround. Update after new market api will aprear.
                //                let agreement_proposal = self.api.provider()
                //                    .get_proposal(subscription_id, demand.id)
                //                    .await?;

                let offer = Proposal::new("".to_string(), serde_json::json!({}), "".to_string());
                let demand = Proposal::new("".to_string(), serde_json::json!({}), "".to_string());
                let agreement_proposal = AgreementProposal::new("".to_string(), demand, offer);

                self.process_agreement(subscription_id, agreement_proposal, &agreement_id)
                    .await?;
            }
        }
        Ok(())
    }

    async fn process_proposal(
        &self,
        subscription_id: &str,
        proposal: AgreementProposal,
    ) -> Result<()> {
        let response = self.negotiator.react_to_proposal(&proposal);
        match response {
            Ok(action) => match action {
                ProposalResponse::AcceptProposal => {
                    self.accept_proposal(subscription_id, &proposal).await?
                }
                ProposalResponse::CounterProposal { proposal } => {
                    self.counter_proposal(subscription_id, proposal).await?
                }
                ProposalResponse::IgnoreProposal => info!("Ignoring proposal {}.", proposal.id),
                ProposalResponse::RejectProposal => {
                    self.reject_proposal(subscription_id, &proposal).await?
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
        &self,
        subscription_id: &str,
        agreement: AgreementProposal,
        agreement_id: &str,
    ) -> Result<()> {
        let response = self.negotiator.react_to_agreement(&agreement);
        match response {
            Ok(action) => match action {
                AgreementResponse::ApproveAgreement => {
                    self.approve_agreement(subscription_id, agreement_id).await?
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

    async fn accept_proposal(
        &self,
        subscription_id: &str,
        proposal: &AgreementProposal,
    ) -> Result<()> {
        info!(
            "Accepting proposal [{}] without changes, subscription_id: {}.",
            proposal.id, subscription_id
        );

        // Note: Provider can't create agreement - only requestor can. We can accept
        // proposal, by resending the same offer as we got from requestor.
        self.api.provider()
            .create_proposal(&proposal.offer, subscription_id, &proposal.id)
            .await?;
        Ok(())
    }

    async fn counter_proposal(&self, subscription_id: &str, proposal: Proposal) -> Result<()> {
        info!(
            "Sending counter offer to proposal [{}], subscription_id: {}.",
            proposal.id, subscription_id
        );

        self.api.provider()
            .create_proposal(&proposal, subscription_id, &proposal.id)
            .await?;
        Ok(())
    }

    async fn reject_proposal(
        &self,
        subscription_id: &str,
        proposal: &AgreementProposal,
    ) -> Result<()> {
        info!(
            "Rejecting proposal [{}], subscription_id: {}.",
            proposal.id, subscription_id
        );

        self.api.provider()
            .reject_proposal(subscription_id, &proposal.id)
            .await?;
        Ok(())
    }

    async fn approve_agreement(&self, subscription_id: &str, agreement_id: &str) -> Result<()> {
        info!(
            "Accepting agreement [{}], subscription_id: {}.",
            agreement_id, subscription_id
        );

        self.api.provider().approve_agreement(agreement_id).await?;
        Ok(())
    }

    async fn reject_agreement(&self, subscription_id: &str, agreement_id: &str) -> Result<()> {
        info!(
            "Rejecting agreement [{}], subscription_id: {}.",
            agreement_id, subscription_id
        );

        self.api.provider().reject_agreement(agreement_id).await?;
        Ok(())
    }
}

// =========================================== //
// Actix stuff
// =========================================== //

impl Actor for ProviderMarket {
    type Context = Context<Self>;
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
