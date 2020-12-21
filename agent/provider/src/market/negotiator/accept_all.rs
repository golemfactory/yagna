use ya_agreement_utils::{AgreementView, OfferDefinition};
use ya_client_model::market::{NewOffer, Proposal};

use super::common::offer_definition_to_offer;
use super::common::{AgreementResponse, AgreementResult, Negotiator, ProposalResponse};

use anyhow::Result;

#[derive(Debug)]
pub struct AcceptAllNegotiator;

impl Negotiator for AcceptAllNegotiator {
    fn create_offer(&mut self, offer: &OfferDefinition) -> Result<NewOffer> {
        Ok(offer_definition_to_offer(offer.clone()))
    }

    fn agreement_finalized(
        &mut self,
        _agreement_id: &str,
        _result: &AgreementResult,
    ) -> Result<()> {
        Ok(())
    }

    fn react_to_proposal(
        &mut self,
        _offer: &NewOffer,
        _demand: &Proposal,
    ) -> Result<ProposalResponse> {
        Ok(ProposalResponse::AcceptProposal)
    }

    fn react_to_agreement(&mut self, _agreement: &AgreementView) -> Result<AgreementResponse> {
        Ok(AgreementResponse::ApproveAgreement)
    }
}

impl AcceptAllNegotiator {
    pub fn new() -> AcceptAllNegotiator {
        AcceptAllNegotiator {}
    }
}
