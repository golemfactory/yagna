use ya_agent_offer_model::OfferDefinition;
use ya_model::market::{AgreementProposal, Offer};

use super::negotiator::Negotiator;
use crate::market::negotiator::{AgreementResponse, ProposalResponse};

use anyhow::{Error, Result};


pub struct AcceptAllNegotiator;

impl Negotiator for AcceptAllNegotiator {
    fn create_offer(&mut self, offer: &OfferDefinition) -> Result<Offer> {
        Ok(Offer::new(offer.clone().into_json(), "()".into()))
    }

    fn react_to_proposal(&mut self, _proposal: &AgreementProposal) -> Result<ProposalResponse> {
        Ok(ProposalResponse::AcceptProposal)
    }

    fn react_to_agreement(&mut self, _agreement: &AgreementProposal) -> Result<AgreementResponse> {
        Ok(AgreementResponse::ApproveAgreement)
    }
}

impl AcceptAllNegotiator {
    pub fn new() -> AcceptAllNegotiator {
        AcceptAllNegotiator {}
    }
}
