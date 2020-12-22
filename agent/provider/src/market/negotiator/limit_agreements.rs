use actix::{Actor, Addr, Context, Handler};
use anyhow::Result;
use chrono::{DateTime, Duration, TimeZone, Utc};
use std::collections::HashSet;

use ya_agreement_utils::agreement::expand;
use ya_client_model::market::{NewOffer, Proposal, Reason};

use super::common::offer_definition_to_offer;
use super::common::{AgreementResponse, Negotiator, ProposalResponse};
use crate::market::negotiator::common::{
    AgreementFinalized, CreateOffer, ReactToAgreement, ReactToProposal,
};
use crate::market::negotiator::factory::LimitAgreementsNegotiatorConfig;
use crate::market::ProviderMarket;

/// Negotiator that can limit number of running agreements.
pub struct LimitAgreementsNegotiator {
    active_agreements: HashSet<String>,
    max_agreements: u32,
}

impl LimitAgreementsNegotiator {
    pub fn new(
        _market: Addr<ProviderMarket>,
        config: &LimitAgreementsNegotiatorConfig,
    ) -> LimitAgreementsNegotiator {
        LimitAgreementsNegotiator {
            max_agreements: config.max_simultaneous_agreements,
            active_agreements: HashSet::new(),
        }
    }

    pub fn has_free_slot(&self) -> bool {
        self.active_agreements.len() < self.max_agreements as usize
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

impl Handler<CreateOffer> for LimitAgreementsNegotiator {
    type Result = anyhow::Result<NewOffer>;

    fn handle(&mut self, msg: CreateOffer, _: &mut Context<Self>) -> Self::Result {
        Ok(offer_definition_to_offer(msg.offer_definition))
    }
}

impl Handler<ReactToProposal> for LimitAgreementsNegotiator {
    type Result = anyhow::Result<ProposalResponse>;

    fn handle(&mut self, msg: ReactToProposal, _: &mut Context<Self>) -> Self::Result {
        let expiration = proposal_expiration_from(&msg.demand)?;
        let min_expiration = Utc::now() + Duration::minutes(5);
        let max_expiration = Utc::now() + Duration::minutes(30);

        if expiration > max_expiration || expiration < min_expiration {
            log::info!(
                "Negotiator: Reject proposal [{:?}] due to expiration limits.",
                msg.demand.proposal_id
            );
            Ok(ProposalResponse::RejectProposal {
                reason: Some(Reason::new(format!(
                    "Proposal expires at: {} which is less than 5 min or more than 30 min from now",
                    expiration
                ))),
            })
        } else if self.has_free_slot() {
            Ok(ProposalResponse::AcceptProposal)
        } else {
            log::info!(
                "Negotiator: Reject proposal [{:?}] due to limit.",
                msg.demand.proposal_id
            );
            Ok(ProposalResponse::RejectProposal {
                reason: Some(Reason::new(format!(
                    "No capacity available. Reached Agreements limit: {}",
                    self.max_agreements
                ))),
            })
        }
    }
}

impl Handler<ReactToAgreement> for LimitAgreementsNegotiator {
    type Result = anyhow::Result<AgreementResponse>;

    fn handle(&mut self, msg: ReactToAgreement, _: &mut Context<Self>) -> Self::Result {
        if self.has_free_slot() {
            self.active_agreements
                .insert(msg.agreement.agreement_id.clone());
            Ok(AgreementResponse::ApproveAgreement)
        } else {
            log::info!(
                "Negotiator: Reject agreement proposal [{}] due to limit.",
                msg.agreement.agreement_id
            );
            Ok(AgreementResponse::RejectAgreement {
                reason: Some(Reason::new(format!(
                    "No capacity available. Reached Agreements limit: {}",
                    self.max_agreements
                ))),
            })
        }
    }
}

impl Handler<AgreementFinalized> for LimitAgreementsNegotiator {
    type Result = anyhow::Result<()>;

    fn handle(&mut self, msg: AgreementFinalized, _: &mut Context<Self>) -> Self::Result {
        self.active_agreements.remove(&msg.agreement_id);

        let free_slots = self.max_agreements as usize - self.active_agreements.len();
        log::info!("Negotiator: {} free slot(s) for agreements.", free_slots);
        Ok(())
    }
}

impl Negotiator for LimitAgreementsNegotiator {}
impl Actor for LimitAgreementsNegotiator {
    type Context = Context<Self>;
}
