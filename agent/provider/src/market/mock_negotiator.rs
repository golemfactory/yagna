use ya_agent_offer_model::OfferDefinition;
use ya_model::market::{Agreement, Offer, Proposal};

use super::negotiator::Negotiator;
use crate::market::negotiator::{AgreementResponse, ProposalResponse};

use anyhow::Result;

#[derive(Debug)]
pub struct AcceptAllNegotiator;

impl Negotiator for AcceptAllNegotiator {
    fn create_offer(&mut self, offer: &OfferDefinition) -> Result<Offer> {
        Ok(Offer::new(
            offer.clone().into_json(),
            r#"(&
                (golem.srv.comp.wasm.task_package=http://localhost:8000/rust-wasi-tutorial.zip)
            )"#
            .into(),
        ))
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
