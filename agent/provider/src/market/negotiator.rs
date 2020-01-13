use ya_model::market::{Offer, Demand, AgreementProposal, Proposal};
use ya_client::{Result,};



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
    fn create_offer(&self) -> Result< Offer >;

    fn react_to_proposal(&self, demand: &Demand) -> Result<ProposalResponse>;
    fn react_to_agreement(&self, proposal: &AgreementProposal) -> Result<AgreementResponse>;
}
