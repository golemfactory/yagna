use actix::prelude::*;
use actix::{Actor, Handler};
use anyhow::Result;
use derive_more::Display;

use ya_agreement_utils::{AgreementView, OfferDefinition};
use ya_client::model::market::Reason;
use ya_client_model::market::{NewOffer, Proposal};

use crate::market::termination_reason::BreakReason;

/// Response for requestor proposals.
#[derive(Debug, Display)]
#[allow(dead_code)]
pub enum ProposalResponse {
    #[display(fmt = "CounterProposal")]
    CounterProposal {
        offer: NewOffer,
    },
    AcceptProposal,
    #[display(
        fmt = "RejectProposal{}",
        "reason.as_ref().map(|r| format!(\" (reason: {})\", r)).unwrap_or(\"\".into())"
    )]
    RejectProposal {
        reason: Option<Reason>,
    },
    ///< Don't send any message to requestor. Could be useful to wait for other offers.
    IgnoreProposal,
}

/// Response for requestor agreements.
#[derive(Debug, Display)]
#[allow(dead_code)]
pub enum AgreementResponse {
    ApproveAgreement,
    #[display(
        fmt = "RejectAgreement{}",
        "reason.as_ref().map(|r| format!(\" (reason: {})\", r)).unwrap_or(\"\".into())"
    )]
    RejectAgreement {
        reason: Option<Reason>,
    },
}

/// Result of agreement execution.
#[derive(Clone)]
pub enum AgreementResult {
    /// Failed to approve agreement. (Agreement even wasn't created)
    ApprovalFailed,
    /// Agreement was finished with success after first Activity.
    ClosedByUs,
    /// Agreement was finished with success by Requestor.
    ClosedByRequestor,
    /// Agreement was broken by us.
    Broken { reason: BreakReason },
}

// =========================================== //
// Negotiator interface
// =========================================== //

/// Negotiator can modify offer, that was generated for him. He can save
/// information about this offer, that are necessary for negotiations.
#[derive(Message)]
#[rtype(result = "Result<NewOffer>")]
pub struct CreateOffer {
    pub offer_definition: OfferDefinition,
}

/// Reactions to events from market. These function make market decisions
/// related to incoming Proposals.
#[derive(Message)]
#[rtype(result = "Result<ProposalResponse>")]
pub struct ReactToProposal {
    pub prev_proposal: Proposal,
    pub demand: Proposal,
}

/// Reactions to events from market. These function make market decisions
/// related to incoming Agreements.
#[derive(Message)]
#[rtype(result = "Result<AgreementResponse>")]
pub struct ReactToAgreement {
    pub agreement: AgreementView,
}

/// Agreement finished notifications. Negotiator can adjust his strategy based on it.
#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct AgreementFinalized {
    pub agreement_id: String,
    pub result: AgreementResult,
}

/// Actor implementing Negotiation logic.
///
/// Direction:
/// - Negotiator should asynchronously generate negotiation decisions instead
///   of returning them as direct response to incoming events. This would allow use
///   to implement time dependent logic like: Collect Proposals during `n` seconds
///   and choose the best from them.
/// - Extensibility: we expect, that developers will implement different market strategies.
///   In best case they should be able to do this without modifying `ya-provider` code.
///   This mean we should implement plugin-like system to communicate with external applications/code.
/// - Multiple negotiating plugins cooperating with each other. Note that introducing new features to
///   Agreement specification requires implementing separate negotiation logic. In this case we
///   can end up with explosion of combination to implement. What worse, we will force external
///   developers to adjust their logic to new Agreement features each time, when they appear.
///   To avoid this we should design internal interfaces, which will allow to combine multiple logics
///   as plugable components.
pub trait Negotiator:
    Actor
    + Handler<CreateOffer, Result = <CreateOffer as Message>::Result>
    + Handler<AgreementFinalized, Result = <AgreementFinalized as Message>::Result>
    + Handler<ReactToProposal, Result = <ReactToProposal as Message>::Result>
    + Handler<ReactToAgreement, Result = <ReactToAgreement as Message>::Result>
{
}

pub fn offer_definition_to_offer(offer_def: OfferDefinition) -> NewOffer {
    let constraints = offer_def.offer.constraints.clone();
    NewOffer::new(offer_def.into_json(), constraints)
}

#[derive(Clone)]
pub struct NegotiatorAddr {
    pub on_create: Recipient<CreateOffer>,
    pub on_finalized: Recipient<AgreementFinalized>,
    pub on_proposal: Recipient<ReactToProposal>,
    pub on_agreement: Recipient<ReactToAgreement>,
}

impl NegotiatorAddr {
    pub async fn create_offer(&self, offer_definition: &OfferDefinition) -> Result<NewOffer> {
        self.on_create
            .send(CreateOffer {
                offer_definition: offer_definition.clone(),
            })
            .await?
    }

    pub async fn react_to_proposal(
        &self,
        prev_proposal: Proposal,
        demand: Proposal,
    ) -> Result<ProposalResponse> {
        self.on_proposal
            .send(ReactToProposal {
                demand,
                prev_proposal,
            })
            .await?
    }

    pub async fn react_to_agreement(
        &self,
        agreement_view: &AgreementView,
    ) -> Result<AgreementResponse> {
        self.on_agreement
            .send(ReactToAgreement {
                agreement: agreement_view.clone(),
            })
            .await?
    }

    pub async fn agreement_finalized(
        &self,
        agreement_id: &str,
        result: AgreementResult,
    ) -> Result<()> {
        self.on_finalized
            .send(AgreementFinalized {
                agreement_id: agreement_id.to_string(),
                result,
            })
            .await?
    }

    pub fn from<T: Negotiator + Actor<Context = Context<T>>>(negotiator: T) -> NegotiatorAddr {
        let addr = negotiator.start();
        NegotiatorAddr {
            on_create: addr.clone().recipient(),
            on_finalized: addr.clone().recipient(),
            on_proposal: addr.clone().recipient(),
            on_agreement: addr.recipient(),
        }
    }
}

pub fn reason_with_extra(message: String, extra: serde_json::Value) -> Reason {
    let mut reason = Reason::new(message);
    reason.extra = extra;
    reason
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proposal_response_display() {
        let reason = ProposalResponse::RejectProposal {
            reason: Some("zima".into()),
        };
        let no_reason = ProposalResponse::RejectProposal { reason: None };

        assert_eq!(reason.to_string(), "RejectProposal (reason: 'zima')");
        assert_eq!(no_reason.to_string(), "RejectProposal");
    }

    #[test]
    fn test_agreement_response_display() {
        let reason = AgreementResponse::RejectAgreement {
            reason: Some("lato".into()),
        };
        let no_reason = AgreementResponse::RejectAgreement { reason: None };

        assert_eq!(reason.to_string(), "RejectAgreement (reason: 'lato')");
        assert_eq!(no_reason.to_string(), "RejectAgreement");
    }
}
