use ya_agent_offer_model::OfferDefinition;
use ya_model::market::{Agreement, Offer, Proposal};

use super::negotiator::Negotiator;
use crate::market::negotiator::{AgreementResponse, ProposalResponse};
use crate::payments::LinearPricingOffer;

use anyhow::Result;

pub struct AcceptAllNegotiator;

impl Negotiator for AcceptAllNegotiator {
    fn create_offer(&mut self, offer: &OfferDefinition) -> Result<Offer> {
        let com_info = LinearPricingOffer::new()
            .add_coefficient("golem.usage.duration_sec", 0.01)
            .add_coefficient("golem.usage.cpu_sec", 0.016)
            .initial_cost(0.02)
            .interval(60.0)
            .build();

        let mut offer = offer.clone();
        offer.com_info = com_info;

        Ok(Offer::new(offer.into_json(), "()".into()))
    }

    fn react_to_proposal(&mut self, demand: &Proposal, offer: &Offer) -> Result<ProposalResponse> {
        log::info!("Accepting proposal: {}", demand.proposal_id()?);
        Ok(ProposalResponse::CounterProposal {
            offer: Proposal::from_offer(demand, offer),
        })
    }

    fn react_to_agreement(&mut self, _agreement: &Agreement) -> Result<AgreementResponse> {
        Ok(AgreementResponse::ApproveAgreement)
    }
}

impl AcceptAllNegotiator {
    pub fn new() -> AcceptAllNegotiator {
        AcceptAllNegotiator {}
    }
}
