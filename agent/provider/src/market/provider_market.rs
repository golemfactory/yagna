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

pub struct ProviderMarket {
    negotiator: Box<dyn Negotiator>,
    api: ApiClient,
    offers: Vec<OfferSubscription>,
}


impl ProviderMarket {

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

    pub async fn run_step(&self) -> Result<()> {

        for offer in self.offers.iter() {
            let events = self.query_events(&offer.subscription_id).await?;
            self.dispatch_events(&offer.subscription_id, &events);
        }

        Ok(())
    }

    async fn query_events(&self, subscription_id: &str) -> Result<Vec<ProviderEvent>> {
        self.api.provider()
            .collect(subscription_id, Some(1), Some(2))
            .await
    }

    async fn dispatch_events(&self, subscription_id: &str, events: &Vec<ProviderEvent>) {
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
                let agreement_proposal = self.api.provider()
                    .get_proposal(subscription_id, proposal_id)
                    .await?;

                self.process_proposal(agreement_proposal);
            },
            ProviderEvent::NewAgreementEvent { agreement_id, .. } => {

            }
        }

        unimplemented!()
    }

    fn process_proposal(&self, proposal: AgreementProposal) {
        let response = self.negotiator.react_to_proposal(&proposal);
        match response {
            Ok(action) => {
                match action {
                    ProposalResponse::CounterProposal{proposal} => self.counter_proposal(proposal),
                    ProposalResponse::IgnoreProposal => info!("Ignoring proposal {}.", proposal.id),
                    ProposalResponse::RejectProposal => self.reject_proposal(&proposal)
                }
            },
            Err(error) => error!("Negotiator error while processing proposal {}.", proposal.id)
        }
    }

    fn process_agreement(&self, agreement: Agreement) {
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

    fn counter_proposal(&self, proposal: Proposal) {
        unimplemented!()
    }

    fn reject_proposal(&self, proposal: &AgreementProposal) {
        unimplemented!()
    }

    fn accept_agreement(&self) {
        unimplemented!()
    }

    fn reject_agreement(&self) {
        unimplemented!()
    }
}


fn create_negotiator(name: &str) -> Box<dyn Negotiator> {
    match name {
        "AcceptAll" => Box::new(AcceptAllNegotiator::new()),
        _ => {
            warn!("Unknown negotiator type {}. Using default: AcceptAll", name);
            Box::new(AcceptAllNegotiator::new())
        }
    }
}
