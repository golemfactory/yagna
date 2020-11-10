use ya_agreement_utils::AgreementView;
use ya_agreement_utils::OfferDefinition;
use ya_client_model::market::{DemandOfferBase, Proposal};

use anyhow::Result;
use derive_more::Display;

use crate::task_state::BreakReason;

/// Response for requestor proposals.
#[derive(Debug, Display)]
#[allow(dead_code)]
pub enum ProposalResponse {
    #[display(fmt = "CounterProposal")]
    CounterProposal {
        offer: DemandOfferBase,
    },
    AcceptProposal,
    #[display(fmt = "RejectProposal( reason: {:?})", reason)]
    RejectProposal {
        reason: Option<String>,
    },
    ///< Don't send any message to requestor. Could be useful to wait for other offers.
    IgnoreProposal,
}

/// Response for requestor agreements.
#[derive(Debug, Display)]
#[allow(dead_code)]
pub enum AgreementResponse {
    ApproveAgreement,
    #[display(fmt = "RejectAgreement( reason: {:?})", reason)]
    RejectAgreement {
        reason: Option<String>,
    },
}

/// Result of agreement execution.
pub enum AgreementResult {
    /// Failed to approve agreement.
    ApprovalFailed,
    /// Agreement was finished with success.
    Closed,
    /// Agreement was broken by us.
    Broken { reason: BreakReason },
}

pub trait Negotiator {
    /// Negotiator can modify offer, that was generated for him. He can save
    /// information about this offer, that are necessary for negotiations.
    fn create_offer(&mut self, node_info: &OfferDefinition) -> Result<DemandOfferBase>;

    /// Agreement notifications. Negotiator can adjust his strategy based on it.
    fn agreement_finalized(&mut self, agreement_id: &str, result: AgreementResult) -> Result<()>;

    /// Reactions to events from market. These function make market decisions.
    fn react_to_proposal(
        &mut self,
        offer: &DemandOfferBase,
        demand: &Proposal,
    ) -> Result<ProposalResponse>;
    fn react_to_agreement(&mut self, agreement: &AgreementView) -> Result<AgreementResponse>;
}
