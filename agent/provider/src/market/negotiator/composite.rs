use actix::{Actor, Addr, Context, Handler};
use anyhow::anyhow;
use serde_json::Value;
use std::convert::TryFrom;

use ya_agreement_utils::agreement::{expand, flatten_value};
use ya_agreement_utils::{AgreementView, OfferTemplate};
use ya_client::model::market::NewOffer;
use ya_client_model::market::proposal::State;

use super::builtin::{
    DebitNoteInterval, LimitExpiration, ManifestSignature, MaxAgreements, PaymentTimeout,
};
use super::common::{offer_definition_to_offer, AgreementResponse, Negotiator, ProposalResponse};
use super::{NegotiationResult, NegotiatorsPack};
use crate::market::negotiator::builtin::demand_validation::DemandValidation;
use crate::market::negotiator::common::{
    reason_with_extra, AgreementFinalized, CreateOffer, ReactToAgreement, ReactToProposal,
};
use crate::market::negotiator::factory::CompositeNegotiatorConfig;
use crate::market::negotiator::{NegotiatorComponent, ProposalView};
use crate::market::ProviderMarket;
use crate::provider_agent::AgentNegotiatorsConfig;

/// Negotiator that can limit number of running agreements.
pub struct CompositeNegotiator {
    components: NegotiatorsPack,
}

impl CompositeNegotiator {
    pub fn new(
        _market: Addr<ProviderMarket>,
        config: &CompositeNegotiatorConfig,
        agent_negotiators_cfg: AgentNegotiatorsConfig,
    ) -> anyhow::Result<CompositeNegotiator> {
        let components = NegotiatorsPack::default()
            .add_component(
                "Validation",
                Box::new(DemandValidation::new(&config.validation_config)),
            )
            .add_component(
                "LimitAgreements",
                Box::new(MaxAgreements::new(&config.limit_agreements_config)),
            )
            .add_component(
                "LimitExpiration",
                Box::new(LimitExpiration::new(&config.expire_agreements_config)?),
            )
            .add_component(
                "DebitNoteInterval",
                Box::new(DebitNoteInterval::new(&config.debit_note_interval_config)?),
            )
            .add_component(
                "PaymentTimeout",
                Box::new(PaymentTimeout::new(&config.payment_timeout_config)?),
            )
            .add_component(
                "ManifestSignature",
                Box::new(ManifestSignature::new(
                    &config.policy_config.clone(),
                    agent_negotiators_cfg,
                )),
            );

        Ok(CompositeNegotiator { components })
    }
}

impl Handler<CreateOffer> for CompositeNegotiator {
    type Result = anyhow::Result<NewOffer>;

    fn handle(&mut self, msg: CreateOffer, _: &mut Context<Self>) -> Self::Result {
        let offer = self.components.fill_template(msg.offer_definition)?;
        Ok(offer_definition_to_offer(offer))
    }
}

impl Handler<ReactToProposal> for CompositeNegotiator {
    type Result = anyhow::Result<ProposalResponse>;

    fn handle(&mut self, msg: ReactToProposal, _: &mut Context<Self>) -> Self::Result {
        // In current implementation we don't allow to change constraints, so we take
        // them from initial Offer.
        let constraints = msg.prev_proposal.constraints;

        let their = ProposalView::try_from(&msg.demand)?;
        let template = ProposalView {
            content: OfferTemplate {
                properties: expand(msg.prev_proposal.properties),
                constraints: constraints.clone(),
            },
            id: msg.prev_proposal.proposal_id,
            issuer: msg.prev_proposal.issuer_id,
            state: msg.prev_proposal.state,
            timestamp: msg.prev_proposal.timestamp,
        };

        let result = self.components.negotiate_step(&their, template)?;
        match result {
            NegotiationResult::Reject { message, is_final } => {
                Ok(ProposalResponse::RejectProposal {
                    reason: Some(reason_with_extra(
                        message,
                        serde_json::json!({ "golem.proposal.rejection.is-final": is_final }),
                    )),
                    is_final,
                })
            }
            NegotiationResult::Ready { offer } | NegotiationResult::Negotiating { offer } => {
                let offer = NewOffer {
                    properties: flatten_value(offer.content.properties),
                    constraints,
                };
                Ok(ProposalResponse::CounterProposal { offer })
            }
        }
    }
}

pub fn to_proposal_views(
    mut agreement: AgreementView,
) -> anyhow::Result<(ProposalView, ProposalView)> {
    // Dispatch Agreement into separate Demand-Offer Proposal pair.
    let offer_id = agreement.pointer_typed("/offer/offerId")?;
    let demand_id = agreement.pointer_typed("/demand/demandId")?;
    let offer_proposal = agreement
        .json
        .pointer_mut("/offer/properties")
        .map(Value::take)
        .unwrap_or(Value::Null);

    let demand_proposal = agreement
        .json
        .pointer_mut("/demand/properties")
        .map(Value::take)
        .unwrap_or(Value::Null);

    let offer_proposal = ProposalView {
        content: OfferTemplate {
            properties: offer_proposal,
            constraints: agreement.pointer_typed("/offer/constraints")?,
        },
        id: offer_id,
        issuer: agreement.pointer_typed("/offer/providerId")?,
        state: State::Accepted,
        timestamp: agreement.creation_timestamp()?,
    };

    let demand_proposal = ProposalView {
        content: OfferTemplate {
            properties: demand_proposal,
            constraints: agreement.pointer_typed("/demand/constraints")?,
        },
        id: demand_id,
        issuer: agreement.pointer_typed("/demand/requestorId")?,
        state: State::Accepted,
        timestamp: agreement.creation_timestamp()?,
    };
    Ok((demand_proposal, offer_proposal))
}

impl Handler<ReactToAgreement> for CompositeNegotiator {
    type Result = anyhow::Result<AgreementResponse>;

    fn handle(&mut self, msg: ReactToAgreement, _: &mut Context<Self>) -> Self::Result {
        let agreement_id = msg.agreement.id.clone();
        let (demand_proposal, offer_proposal) = to_proposal_views(msg.agreement).map_err(|e| {
            anyhow!(
                "Negotiator failed to extract Proposals from Agreement. {}",
                e
            )
        })?;

        // We expect that all `NegotiatorComponents` should return ready state.
        // Otherwise we must reject Agreement proposals, because negotiations didn't end.
        match self
            .components
            .negotiate_step(&demand_proposal, offer_proposal)?
        {
            NegotiationResult::Ready { .. } => {
                self.components.on_agreement_approved(&agreement_id)?;
                Ok(AgreementResponse::ApproveAgreement)
            }
            NegotiationResult::Reject { message, is_final } => {
                Ok(AgreementResponse::RejectAgreement {
                    reason: Some(reason_with_extra(
                        message,
                        serde_json::json!({ "golem.proposal.rejection.is-final": is_final }),
                    )),
                    is_final,
                })
            }
            NegotiationResult::Negotiating { .. } => Ok(AgreementResponse::RejectAgreement {
                reason: Some(reason_with_extra(
                    "Negotiations aren't finished.".to_string(),
                    serde_json::json!({ "golem.proposal.rejection.is-final": false }),
                )),
                is_final: false,
            }),
        }
    }
}

impl Handler<AgreementFinalized> for CompositeNegotiator {
    type Result = anyhow::Result<()>;

    fn handle(&mut self, msg: AgreementFinalized, _: &mut Context<Self>) -> Self::Result {
        self.components
            .on_agreement_terminated(&msg.agreement_id, &msg.result)
    }
}

impl Negotiator for CompositeNegotiator {}
impl Actor for CompositeNegotiator {
    type Context = Context<Self>;
}
