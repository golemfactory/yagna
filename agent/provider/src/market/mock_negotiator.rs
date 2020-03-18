use ya_agent_offer_model::OfferDefinition;
use ya_model::market::{Agreement, Offer, Proposal};

use super::negotiator::Negotiator;
use crate::market::negotiator::{AgreementResponse, ProposalResponse};
use crate::payments::LinearPricingOffer;

use anyhow::Result;

#[derive(Debug)]
pub struct AcceptAllNegotiator;

impl Negotiator for AcceptAllNegotiator {
    fn create_offer(&mut self, offer: &OfferDefinition) -> Result<Offer> {
        let com_info = LinearPricingOffer::new()
            .add_coefficient("golem.usage.duration_sec", 0.01)
            .add_coefficient("golem.usage.cpu_sec", 0.016)
            .initial_cost(0.02)
            .interval(6.0)
            .build();

        let mut offer = offer.clone();
        offer.com_info = com_info;

        Ok(Offer::new(offer.into_json(), "()".into()))
    }

    fn react_to_proposal(
        &mut self,
        _offer: &Offer,
        _demand: &Proposal,
    ) -> Result<ProposalResponse> {
        Ok(ProposalResponse::AcceptProposal)
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
