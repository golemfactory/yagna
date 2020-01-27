use ya_client::Result;
use ya_model::market::{AgreementProposal, Offer};

use super::negotiator::Negotiator;
use crate::market::negotiator::{AgreementResponse, ProposalResponse};
use crate::node_info::NodeInfo;

use serde_json;

pub struct AcceptAllNegotiator;

impl Negotiator for AcceptAllNegotiator {
    fn create_offer(&self, node_info: &NodeInfo) -> Result<Offer> {
        Ok(Offer::new(serde_json::json!(node_info), "()".into()))
    }

    fn react_to_proposal(&self, _proposal: &AgreementProposal) -> Result<ProposalResponse> {
        Ok(ProposalResponse::AcceptProposal)
    }

    fn react_to_agreement(&self, _agreement: &AgreementProposal) -> Result<AgreementResponse> {
        Ok(AgreementResponse::ApproveAgreement)
    }
}

impl AcceptAllNegotiator {
    pub fn new() -> AcceptAllNegotiator {
        AcceptAllNegotiator {}
    }
}
