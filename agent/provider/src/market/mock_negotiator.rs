use ya_client::Result;
use ya_model::market::{Agreement, Offer, Proposal};

use super::negotiator::Negotiator;
use crate::market::negotiator::{AgreementResponse, ProposalResponse};

use ya_agent_offer_model::OfferDefinition;

pub struct AcceptAllNegotiator;

impl Negotiator for AcceptAllNegotiator {
    fn create_offer(&self, offer: &OfferDefinition) -> Result<Offer> {
        Ok(Offer::new(offer.clone().into_json(), "()".into()))
    }

    fn react_to_proposal(&self, _proposal: &Proposal) -> Result<ProposalResponse> {
        Ok(ProposalResponse::AcceptProposal)
    }

    fn react_to_agreement(&self, _agreement: &Agreement) -> Result<AgreementResponse> {
        Ok(AgreementResponse::ApproveAgreement)
    }
}

impl AcceptAllNegotiator {
    pub fn new() -> AcceptAllNegotiator {
        AcceptAllNegotiator {}
    }
}
