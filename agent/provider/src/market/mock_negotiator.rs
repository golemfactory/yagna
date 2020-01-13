use super::negotiator::{Negotiator};
use ya_model::market::{Demand, Offer, AgreementProposal};
use crate::market::negotiator::{ProposalResponse, AgreementResponse};
use ya_client::Error;


pub struct AcceptAllNegotiator;


impl Negotiator for AcceptAllNegotiator {

    fn create_offer(&self) -> Result<Offer, Error> {
        unimplemented!()
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
