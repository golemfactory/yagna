use ya_agreement_utils::AgreementView;
use ya_agreement_utils::OfferDefinition;
use ya_client_model::market::{NewOffer, Proposal};

use super::negotiator::Negotiator;
use crate::market::negotiator::{AgreementResponse, AgreementResult, ProposalResponse};

use anyhow::Result;
use chrono::{DateTime, Duration, TimeZone, Utc};
use std::collections::HashSet;
use ya_agreement_utils::agreement::expand;

#[derive(Debug)]
pub struct AcceptAllNegotiator;

impl Negotiator for AcceptAllNegotiator {
    fn create_offer(&mut self, offer: &OfferDefinition) -> Result<NewOffer> {
        Ok(offer_definition_to_offer(offer.clone()))
    }

    fn agreement_finalized(&mut self, _agreement_id: &str, _result: AgreementResult) -> Result<()> {
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
    fn create_offer(&mut self, offer: &OfferDefinition) -> Result<NewOffer> {
        Ok(offer_definition_to_offer(offer.clone()))
    }

    fn agreement_finalized(&mut self, agreement_id: &str, _result: AgreementResult) -> Result<()> {
        self.active_agreements.remove(agreement_id);

        let free_slots = self.max_agreements as usize - self.active_agreements.len();
        log::info!("Negotiator: {} free slot(s) for agreements.", free_slots);
        Ok(())
    }

    fn react_to_proposal(
        &mut self,
        _offer: &NewOffer,
        demand: &Proposal,
    ) -> Result<ProposalResponse> {
        let expiration = proposal_expiration_from(&demand)?;
        let min_expiration = Utc::now() + Duration::minutes(5);
        let max_expiration = Utc::now() + Duration::minutes(30);

        if expiration > max_expiration || expiration < min_expiration {
            log::info!(
                "Negotiator: Reject proposal [{:?}] due to expiration limits.",
                demand.proposal_id
            );
            Ok(ProposalResponse::RejectProposal {
                reason: Some(format!(
                    "Proposal expires at: {} which is less than 5 min or more than 30 min from now",
                    expiration
                )),
            })
        } else if self.has_free_slot() {
            Ok(ProposalResponse::AcceptProposal)
        } else {
            log::info!(
                "Negotiator: Reject proposal [{:?}] due to limit.",
                demand.proposal_id
            );
            Ok(ProposalResponse::RejectProposal {
                reason: Some(format!(
                    "No capacity available. Reached Agreements limit: {}",
                    self.max_agreements
                )),
            })
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
            Ok(AgreementResponse::RejectAgreement {
                reason: Some(format!(
                    "No capacity available. Reached Agreements limit: {}",
                    self.max_agreements
                )),
            })
        }
    }
}

fn proposal_expiration_from(proposal: &Proposal) -> Result<DateTime<Utc>> {
    let expiration_key_str = "/golem/srv/comp/expiration";
    let value = expand(proposal.properties.clone())
        .pointer(expiration_key_str)
        .ok_or_else(|| anyhow::anyhow!("Missing expiration key"))?
        .clone();
    let timestamp: i64 = serde_json::from_value(value)?;
    Ok(Utc.timestamp_millis(timestamp))
}

fn offer_definition_to_offer(offer_def: OfferDefinition) -> NewOffer {
    let constraints = offer_def.offer.constraints.clone();
    NewOffer::new(offer_def.into_json(), constraints)
}
