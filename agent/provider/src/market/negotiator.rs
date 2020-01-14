use ya_model::market::{Offer, Demand, AgreementProposal, Proposal, Agreement};
use ya_client::{Result,};

use crate::node_info::{NodeInfo};


/// Response for requestor proposals.
pub enum ProposalResponse {
    CounterProposal {
        proposal: Proposal
    },
    RejectProposal,
    ///< Don't send any message to requestor. Could be useful to wait for other offers.
    IgnoreProposal,
}

/// Response for requestor agreements.
pub enum AgreementResponse {
    AcceptAgreement,
    RejectAgreement,
}


pub trait Negotiator {

    //TODO: We should add some parameters for offer creation.
    fn create_offer(&self, node_info: &NodeInfo) -> Result< Offer >;

    fn react_to_proposal(&self, proposal: &AgreementProposal) -> Result<ProposalResponse>;
    fn react_to_agreement(&self, agreement: &Agreement) -> Result<AgreementResponse>;
}
