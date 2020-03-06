use ya_agent_offer_model::OfferDefinition;
use ya_model::market::{Agreement, Offer, Proposal};

use super::negotiator::Negotiator;
use crate::market::negotiator::{AgreementResponse, ProposalResponse};

use anyhow::Result;

pub struct AcceptAllNegotiator;

impl Negotiator for AcceptAllNegotiator {
    fn create_offer(&mut self, offer: &OfferDefinition) -> Result<Offer> {
        Ok(Offer::new(offer.clone().into_json(), "()".into()))
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
