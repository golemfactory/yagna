use actix::{Actor, Addr, Context, Handler};
use anyhow::anyhow;
use serde_json::Value;

use ya_agreement_utils::agreement::{expand, flatten_value};
use ya_agreement_utils::AgreementView;
use ya_client::model::market::NewOffer;

use super::builtin::{DebitNoteInterval, LimitExpiration, ManifestSignature, MaxAgreements, PaymentTimeout};
use super::common::{offer_definition_to_offer, AgreementResponse, Negotiator, ProposalResponse};
use super::{NegotiationResult, NegotiatorsPack};
use crate::market::negotiator::common::{
    reason_with_extra, AgreementFinalized, CreateOffer, ReactToAgreement, ReactToProposal,
};
use crate::market::negotiator::factory::CompositeNegotiatorConfig;
use crate::market::negotiator::{NegotiatorComponent, ProposalView};
use crate::market::ProviderMarket;

/// Negotiator that can limit number of running agreements.
pub struct CompositeNegotiator {
    components: NegotiatorsPack,
}

impl CompositeNegotiator {
    pub fn new(
        _market: Addr<ProviderMarket>,
        config: &CompositeNegotiatorConfig,
    ) -> anyhow::Result<CompositeNegotiator> {
        let components = NegotiatorsPack::new()
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
                Box::new(ManifestSignature::from(config.policy_config.clone())),
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
        let proposal_constraints = msg.demand.constraints.clone();

        let proposal = ProposalView {
            agreement_id: msg.demand.proposal_id,
            json: expand(msg.demand.properties),
        };

        let offer_proposal = ProposalView {
            json: expand(msg.prev_proposal.properties),
            agreement_id: msg.prev_proposal.proposal_id,
        };

        let result =
            self.components
                .negotiate_step(&proposal, &proposal_constraints, offer_proposal)?;
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
                    properties: flatten_value(offer.json),
                    constraints,
                };
                Ok(ProposalResponse::CounterProposal { offer })
            }
        }
    }
}

pub fn to_proposal_views(
    mut agreement: AgreementView,
) -> anyhow::Result<(ProposalView, String, ProposalView)> {
    // Dispatch Agreement into separate Demand-Offer Proposal pair.
    // TODO: We should get ProposalId here, but Agreement doen't store it anywhere.
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
        json: offer_proposal,
        agreement_id: offer_id,
    };

    let demand_proposal = ProposalView {
        json: demand_proposal,
        agreement_id: demand_id,
    };

    let demand_constraints = agreement
        .json
        .pointer_mut("/demand/properties")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .unwrap_or_default();

    Ok((demand_proposal, demand_constraints, offer_proposal))
}

impl Handler<ReactToAgreement> for CompositeNegotiator {
    type Result = anyhow::Result<AgreementResponse>;

    fn handle(&mut self, msg: ReactToAgreement, _: &mut Context<Self>) -> Self::Result {
        let agreement_id = msg.agreement.agreement_id.clone();
        let (demand_proposal, demand_constraints, offer_proposal) =
            to_proposal_views(msg.agreement).map_err(|e| {
                anyhow!(
                    "Negotiator failed to extract Proposals from Agreement. {}",
                    e
                )
            })?;

        // We expect that all `NegotiatorComponents` should return ready state.
        // Otherwise we must reject Agreement proposals, because negotiations didn't end.
        match self.components.negotiate_step(
            &demand_proposal,
            &demand_constraints,
            offer_proposal,
        )? {
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
                    format!("Negotiations aren't finished."),
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
