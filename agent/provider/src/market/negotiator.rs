use ya_agreement_utils::AgreementView;
use ya_agreement_utils::OfferDefinition;
use ya_client_model::market::{NewOffer, Proposal};

use anyhow::Result;
use derive_more::Display;

use crate::market::termination_reason::BreakReason;
use ya_client::model::market::Reason;

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

pub trait Negotiator {
    /// Negotiator can modify offer, that was generated for him. He can save
    /// information about this offer, that are necessary for negotiations.
    fn create_offer(&mut self, node_info: &OfferDefinition) -> Result<NewOffer>;

    /// Agreement notifications. Negotiator can adjust his strategy based on it.
    fn agreement_finalized(&mut self, agreement_id: &str, result: &AgreementResult) -> Result<()>;

    /// Reactions to events from market. These function make market decisions.
    fn react_to_proposal(
        &mut self,
        offer: &NewOffer,
        demand: &Proposal,
    ) -> Result<ProposalResponse>;
    fn react_to_agreement(&mut self, agreement: &AgreementView) -> Result<AgreementResponse>;
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
