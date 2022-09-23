use actix::{Actor, Context, Handler};

use ya_client::model::market::NewOffer;

use super::common::offer_definition_to_offer;
use super::common::{AgreementResponse, Negotiator, ProposalResponse};
use crate::market::negotiator::common::{
    AgreementFinalized, CreateOffer, ReactToAgreement, ReactToProposal,
};

#[derive(Debug)]
pub struct AcceptAllNegotiator;

impl AcceptAllNegotiator {
    pub fn new() -> AcceptAllNegotiator {
        AcceptAllNegotiator {}
    }
}

impl Handler<CreateOffer> for AcceptAllNegotiator {
    type Result = anyhow::Result<NewOffer>;

    fn handle(&mut self, msg: CreateOffer, _: &mut Context<Self>) -> Self::Result {
        Ok(offer_definition_to_offer(msg.offer_definition))
    }
}

impl Handler<ReactToProposal> for AcceptAllNegotiator {
    type Result = anyhow::Result<ProposalResponse>;

    fn handle(&mut self, _: ReactToProposal, _: &mut Context<Self>) -> Self::Result {
        Ok(ProposalResponse::AcceptProposal)
    }
}

impl Handler<ReactToAgreement> for AcceptAllNegotiator {
    type Result = anyhow::Result<AgreementResponse>;

    fn handle(&mut self, _: ReactToAgreement, _: &mut Context<Self>) -> Self::Result {
        Ok(AgreementResponse::ApproveAgreement)
    }
}

impl Handler<AgreementFinalized> for AcceptAllNegotiator {
    type Result = anyhow::Result<()>;

    fn handle(&mut self, _: AgreementFinalized, _: &mut Context<Self>) -> Self::Result {
        Ok(())
    }
}

impl Negotiator for AcceptAllNegotiator {}
impl Actor for AcceptAllNegotiator {
    type Context = Context<Self>;
}
