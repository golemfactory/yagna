use ya_agreement_utils::AgreementView;
use ya_agreement_utils::OfferDefinition;
use ya_client_model::market::{Offer, Proposal};

use super::negotiator::Negotiator;
use crate::market::negotiator::{AgreementResponse, AgreementResult, ProposalResponse};

use anyhow::Result;
use std::collections::HashSet;

#[derive(Debug)]
pub struct AcceptAllNegotiator;

impl Negotiator for AcceptAllNegotiator {
    fn create_offer(&mut self, offer: &OfferDefinition) -> Result<Offer> {
        Ok(Offer::new(
            offer.clone().into_json(),
            offer.constraints.clone(),
        ))
    }

    fn agreement_finalized(&mut self, _agreement_id: &str, _result: AgreementResult) -> Result<()> {
        Ok(())
    }

    fn react_to_proposal(
        &mut self,
        _offer: &Offer,
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

/// Negotiator that can limit number of running agreements.
pub struct LimitAgreementsNegotiator {
    active_agreements: HashSet<String>,
    max_agreements: u32,
}

impl LimitAgreementsNegotiator {
    pub fn new(max_agreements: u32) -> LimitAgreementsNegotiator {
        LimitAgreementsNegotiator {
            max_agreements,
            active_agreements: HashSet::new(),
        }
    }

    pub fn has_free_slot(&self) -> bool {
        self.active_agreements.len() < self.max_agreements as usize
    }
}

impl Negotiator for LimitAgreementsNegotiator {
    fn create_offer(&mut self, offer: &OfferDefinition) -> Result<Offer> {
        Ok(Offer::new(
            offer.clone().into_json(),
            offer.constraints.clone(),
        ))
    }

    fn agreement_finalized(&mut self, agreement_id: &str, _result: AgreementResult) -> Result<()> {
        self.active_agreements.remove(agreement_id);

        let free_slots = self.max_agreements as usize - self.active_agreements.len();
        log::info!("Negotiator: {} free slot(s) for agreements.", free_slots);
        Ok(())
    }

    fn react_to_proposal(&mut self, _offer: &Offer, demand: &Proposal) -> Result<ProposalResponse> {
        if self.has_free_slot() {
            Ok(ProposalResponse::AcceptProposal)
        } else {
            log::info!(
                "Negotiator: Reject proposal [{:?}] due to limit.",
                demand.proposal_id
            );
            Ok(ProposalResponse::RejectProposal)
        }
    }

    fn react_to_agreement(&mut self, agreement: &AgreementView) -> Result<AgreementResponse> {
        if self.has_free_slot() {
            self.active_agreements
                .insert(agreement.agreement_id.clone());
            Ok(AgreementResponse::ApproveAgreement)
        } else {
            log::info!(
                "Negotiator: Reject agreement proposal [{}] due to limit.",
                agreement.agreement_id
            );
            Ok(AgreementResponse::RejectAgreement)
        }
    }
}
