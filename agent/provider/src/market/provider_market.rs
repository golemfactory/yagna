use super::negotiator::{Negotiator, ProposalResponse, AgreementResponse};
use super::mock_negotiator::{AcceptAllNegotiator};
use crate::node_info::{NodeInfo};

use ya_client::{market::{ApiClient, ProviderApi}, Result, Error};
use ya_model::market::{ProviderEvent, Offer, AgreementProposal, Agreement, Proposal};

use futures::executor::block_on;
use log::{info, warn, error};


struct OfferSubscription {
    subscription_id: String,
    offer: Offer,
}

// Manages market api communication and forwards proposal
// to implementation of market strategy.
pub struct ProviderMarket {
    negotiator: Box<dyn Negotiator>,
    api: ApiClient,
    offers: Vec<OfferSubscription>,
}


impl ProviderMarket {

    // =========================================== //
    // Initialization
    // =========================================== //

    pub fn new(api: ApiClient, negotiator_type: &str) -> ProviderMarket {
        let negotiator = create_negotiator(negotiator_type);
        return ProviderMarket{api, negotiator, offers: vec![]};
    }

    pub async fn create_offers(&mut self, node_info: &NodeInfo) -> Result<()> {
        info!("Creating initial offer.");

        let offer = self.negotiator.create_offer(node_info)?;

        info!("Subscribing to events.");

        let subscription_id = self.api.provider().subscribe(&offer).await?;
        self.offers.push(OfferSubscription{subscription_id, offer});
        Ok(())
    }

    pub async fn onshutdown(&mut self) -> Result<()>{
        info!("Unsubscribing events.");

        for offer in self.offers.iter() {
            self.api.provider().unsubscribe(&offer.subscription_id).await?;
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
        self.api.provider()
            .collect(subscription_id, Some(1), Some(2))
            .await
    }

    async fn dispatch_events(&self, subscription_id: &str, events: &Vec<ProviderEvent>) {
        info!("Collected {} events. Processing...", events.len());

        for event in events.iter() {
            if let Err(error) = self.dispatch_event(subscription_id, event).await {
                error!("Error processing event: {}, subscription_id: {}.", error, subscription_id);
            }
        }
    }

    async fn dispatch_event(&self, subscription_id: &str, event: &ProviderEvent) -> Result<()> {

        match event {
            ProviderEvent::DemandEvent { demand, .. } => {
                let proposal_id = &demand.as_ref().unwrap().id;

                info!("Got demand [id={}].", proposal_id);

                let agreement_proposal = self.api.provider()
                    .get_proposal(subscription_id, proposal_id)
                    .await?;

                self.process_proposal(subscription_id, agreement_proposal).await?;
            },
            ProviderEvent::NewAgreementEvent { agreement_id, .. } => {
                unimplemented!()
            }
        }
        Ok(())
    }

    async fn process_proposal(&self, subscription_id: &str, proposal: AgreementProposal) -> Result<()>  {
        let response = self.negotiator.react_to_proposal(&proposal);
        match response {
            Ok(action) => {
                match action {
                    ProposalResponse::AcceptProposal => self.accept_proposal(subscription_id, &proposal).await?,
                    ProposalResponse::CounterProposal{proposal} => self.counter_proposal(subscription_id, proposal).await?,
                    ProposalResponse::IgnoreProposal => info!("Ignoring proposal {}.", proposal.id),
                    ProposalResponse::RejectProposal => self.reject_proposal(subscription_id, &proposal).await?
                }
            },
            Err(error) => error!("Negotiator error while processing proposal {}.", proposal.id)
        }
        Ok(())
    }

    fn process_agreement(&self, subscription_id: &str, agreement: Agreement) {
        let response = self.negotiator.react_to_agreement(&agreement);
        match response {
            Ok(action) => {
                match action {
                    AgreementResponse::AcceptAgreement => self.accept_agreement(),
                    AgreementResponse::RejectAgreement => self.reject_agreement(),
                }
            },
            Err(error) => error!("Negotiator error while processing agreement {}.", agreement.proposal_id)
        }
    }

    // =========================================== //
    // Market internals - proposals and agreements reactions
    // =========================================== //

    async fn accept_proposal(&self, subscription_id: &str, proposal: &AgreementProposal) -> Result<()> {
        info!("Accepting proposal [{}] without changes.", proposal.id);

        // Note: Provider can't create agreement - only requestor can. We can accept
        // proposal, by resending the same offer as we got from requestor.
        self.api.provider().create_proposal(&proposal.offer, subscription_id, &proposal.id).await?;
        Ok(())
    }

    async fn counter_proposal(&self, subscription_id: &str, proposal: Proposal) -> Result<()> {
        info!("Sending counter offer to proposal [{}]", proposal.id);

        self.api.provider().create_proposal(&proposal, subscription_id, &proposal.id).await?;
        Ok(())
    }

    async fn reject_proposal(&self, subscription_id: &str, proposal: &AgreementProposal) -> Result<()> {
        info!("Rejecting proposal [{}]", proposal.id);

        self.api.provider().reject_proposal(subscription_id, &proposal.id).await?;
        Ok(())
    }

    fn accept_agreement(&self) {
        unimplemented!()
    }

    fn reject_agreement(&self) {
        unimplemented!()
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
