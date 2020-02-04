use ya_agent_offer_model::OfferDefinition;
use ya_model::market::{Agreement, Offer, Proposal};

use anyhow::Result;

/// Response for requestor proposals.
#[allow(dead_code)]
pub enum ProposalResponse {
    CounterProposal {
        proposal: Proposal,
    },
    AcceptProposal,
    RejectProposal,
    ///< Don't send any message to requestor. Could be useful to wait for other offers.
    IgnoreProposal,
}

/// Response for requestor agreements.
#[allow(dead_code)]
pub enum AgreementResponse {
    ApproveAgreement,
    RejectAgreement,
}

pub trait Negotiator {
    //TODO: We should add some parameters for offer creation.
    fn create_offer(&mut self, node_info: &OfferDefinition) -> Result<Offer>;

    fn react_to_proposal(&mut self, proposal: &Proposal) -> Result<ProposalResponse>;
    fn react_to_agreement(&mut self, agreement: &Agreement) -> Result<AgreementResponse>;
}
