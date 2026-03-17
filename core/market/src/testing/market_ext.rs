use anyhow::bail;
use ya_client::model::market::RequestorEvent;

use crate::db::dao::ProposalDao;
use crate::db::model::{Demand, Offer, Proposal, ProposalId, SubscriptionId};
use crate::matcher::error::{DemandError, QueryOfferError};
use crate::negotiation::error::{GetProposalError, QueryEventsError};
use crate::MarketService;

#[async_trait::async_trait]
pub trait MarketServiceExt {
    async fn get_offer(&self, id: &SubscriptionId) -> Result<Offer, QueryOfferError>;
    async fn get_demand(&self, id: &SubscriptionId) -> Result<Demand, DemandError>;
    async fn get_proposal(&self, id: &ProposalId) -> Result<Proposal, GetProposalError>;
    async fn get_proposal_from_db(
        &self,
        proposal_id: &ProposalId,
    ) -> Result<Proposal, anyhow::Error>;
    async fn query_events(
        &self,
        subscription_id: &SubscriptionId,
        timeout: f32,
        max_events: Option<i32>,
    ) -> Result<Vec<RequestorEvent>, QueryEventsError>;
}

#[async_trait::async_trait]
impl MarketServiceExt for MarketService {
    async fn get_offer(&self, id: &SubscriptionId) -> Result<Offer, QueryOfferError> {
        self.matcher.store.get_offer(id).await
    }

    async fn get_demand(&self, id: &SubscriptionId) -> Result<Demand, DemandError> {
        self.matcher.store.get_demand(id).await
    }

    async fn get_proposal(&self, id: &ProposalId) -> Result<Proposal, GetProposalError> {
        self.provider_engine.common.get_proposal(None, id).await
    }

    async fn get_proposal_from_db(
        &self,
        proposal_id: &ProposalId,
    ) -> Result<Proposal, anyhow::Error> {
        let db = self.db.clone();
        Ok(
            match db.as_dao::<ProposalDao>().get_proposal(proposal_id).await? {
                Some(proposal) => proposal,
                None => bail!("Proposal [{}] not found", proposal_id),
            },
        )
    }

    async fn query_events(
        &self,
        subscription_id: &SubscriptionId,
        timeout: f32,
        max_events: Option<i32>,
    ) -> Result<Vec<RequestorEvent>, QueryEventsError> {
        self.requestor_engine
            .query_events(subscription_id, timeout, max_events)
            .await
    }
}
