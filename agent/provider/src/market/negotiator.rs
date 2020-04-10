use ya_agent_offer_model::OfferDefinition;
use ya_model::market::{Agreement, Offer, Proposal};

use anyhow::Result;
use derive_more::Display;

/// Response for requestor proposals.
#[derive(Debug, Display)]
#[allow(dead_code)]
pub enum ProposalResponse {
    #[display(fmt = "CounterProposal")]
    CounterProposal {
        offer: Proposal,
    },
    AcceptProposal,
    RejectProposal,
    ///< Don't send any message to requestor. Could be useful to wait for other offers.
    IgnoreProposal,
}

/// Response for requestor agreements.
#[derive(Debug, Display)]
#[allow(dead_code)]
pub enum AgreementResponse {
    ApproveAgreement,
    RejectAgreement,
}

pub trait Negotiator: std::fmt::Debug {
    /// Negotiator can modify offer, that was generated for him. He can save
    /// information about this offer, that are necessary for negotiations.
    fn create_offer(&mut self, node_info: &OfferDefinition) -> Result<Offer>;
    fn react_to_proposal(&mut self, offer: &Offer, demand: &Proposal) -> Result<ProposalResponse>;
    fn react_to_agreement(&mut self, agreement: &Agreement) -> Result<AgreementResponse>;
}
