use ya_model::market::{Demand, Offer, AgreementProposal};
use ya_client::Error;

use super::negotiator::{Negotiator};
use crate::market::negotiator::{ProposalResponse, AgreementResponse};
use crate::node_info::{NodeInfo, CpuInfo};

use serde::{Serialize};
use serde_json;



pub struct AcceptAllNegotiator;


impl Negotiator for AcceptAllNegotiator {

    fn create_offer(&self, node_info: &NodeInfo) -> Result<Offer, Error> {
        Ok(Offer::new(serde_json::json!(node_info), "()".into()))
    }

    fn react_to_proposal(&self, _demand: &Demand) -> Result<ProposalResponse, Error> {
        Ok(ProposalResponse::RejectProposal)
    }

    fn react_to_agreement(&self, _proposal: &AgreementProposal) -> Result<AgreementResponse, Error> {
        Ok(AgreementResponse::AcceptAgreement)
    }
}

impl AcceptAllNegotiator {

    pub fn new() -> AcceptAllNegotiator {
        AcceptAllNegotiator{}
    }
}
