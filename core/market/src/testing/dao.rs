use async_trait::async_trait;
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};
use ya_persistence::executor::{do_with_transaction, PoolType};

use crate::db::model::{
    Agreement, DbProposal, Demand, Negotiation, Offer, ProposalId, SubscriptionId,
};
use crate::db::schema::market_agreement::dsl as agreement_dsl;
use crate::db::schema::market_demand::dsl as demand_dsl;
use crate::db::schema::market_negotiation::dsl as negotiation_dsl;
use crate::db::schema::market_negotiation_event::dsl as event_dsl;
use crate::db::schema::market_offer::dsl as offer_dsl;
use crate::db::schema::market_proposal::dsl as proposal_dsl;
use crate::db::{DbError, DbResult};
use crate::testing::events_helper::TestMarketEvent;

#[async_trait]
pub trait TestingDao<M: 'static + Send> {
    type IdType: 'static + Send;
    async fn get_by_id(&self, id: Self::IdType) -> DbResult<Option<M>>;
    async fn exists(&self, id: Self::IdType) -> bool {
        match self.get_by_id(id).await {
            Err(_e) => false,
            Ok(optional) => optional.is_some(),
        }
    }
    async fn raw_insert(&self, _instance: M) -> DbResult<()> {
        Ok(())
    }
}

#[async_trait]
impl TestingDao<Demand> for PoolType {
    type IdType = SubscriptionId;
    async fn get_by_id(&self, id: SubscriptionId) -> DbResult<Option<Demand>> {
        do_with_transaction(self, "testing_dao_demand_get_by_id", move |conn| {
            Ok(demand_dsl::market_demand
                .filter(demand_dsl::id.eq(id))
                .first(conn)
                .optional()?)
        })
        .await
    }
}

#[async_trait]
impl TestingDao<Agreement> for PoolType {
    type IdType = ProposalId;
    async fn get_by_id(&self, id: ProposalId) -> DbResult<Option<Agreement>> {
        do_with_transaction(self, "testing_dao_agreement_get_by_id", move |conn| {
            Ok(agreement_dsl::market_agreement
                .filter(agreement_dsl::id.eq(id))
                .first(conn)
                .optional()?)
        })
        .await
    }
}

#[async_trait]
impl TestingDao<Offer> for PoolType {
    type IdType = SubscriptionId;
    async fn get_by_id(&self, id: SubscriptionId) -> DbResult<Option<Offer>> {
        do_with_transaction(self, "testing_dao_offer_get_by_id", move |conn| {
            Ok(offer_dsl::market_offer
                .filter(offer_dsl::id.eq(id))
                .first(conn)
                .optional()?)
        })
        .await
    }
}

#[async_trait]
impl TestingDao<DbProposal> for PoolType {
    type IdType = ProposalId;
    async fn get_by_id(&self, id: ProposalId) -> DbResult<Option<DbProposal>> {
        do_with_transaction(self, "testing_dao_db_proposal_get_by_id", move |conn| {
            Ok(proposal_dsl::market_proposal
                .filter(proposal_dsl::id.eq(id))
                .first(conn)
                .optional()?)
        })
        .await
    }

    async fn raw_insert(&self, instance: DbProposal) -> DbResult<()> {
        do_with_transaction(self, "testing_dao_db_proposal_raw_insert", move |conn| {
            Result::<usize, DbError>::Ok(
                diesel::insert_into(proposal_dsl::market_proposal)
                    .values(instance)
                    .execute(conn)?,
            )
        })
        .await?;
        Ok(())
    }
}

#[async_trait]
impl TestingDao<Negotiation> for PoolType {
    type IdType = String;
    async fn get_by_id(&self, id: String) -> DbResult<Option<Negotiation>> {
        do_with_transaction(self, "testing_dao_negotiation_get_by_id", move |conn| {
            Ok(negotiation_dsl::market_negotiation
                .filter(negotiation_dsl::id.eq(id))
                .first(conn)
                .optional()?)
        })
        .await
    }

    async fn raw_insert(&self, instance: Negotiation) -> DbResult<()> {
        do_with_transaction(self, "testing_dao_negotiation_raw_insert", move |conn| {
            Result::<usize, DbError>::Ok(
                diesel::insert_into(negotiation_dsl::market_negotiation)
                    .values(instance)
                    .execute(conn)?,
            )
        })
        .await?;
        Ok(())
    }
}

#[async_trait]
impl TestingDao<TestMarketEvent> for PoolType {
    type IdType = i32;
    async fn get_by_id(&self, id: i32) -> DbResult<Option<TestMarketEvent>> {
        do_with_transaction(self, "testing_dao_market_event_get_by_id", move |conn| {
            Ok(event_dsl::market_negotiation_event
                .filter(event_dsl::id.eq(id))
                .first(conn)
                .optional()?)
        })
        .await
    }
    async fn raw_insert(&self, instance: TestMarketEvent) -> DbResult<()> {
        do_with_transaction(self, "testing_dao_market_event_raw_insert", move |conn| {
            Result::<usize, DbError>::Ok(
                diesel::insert_into(event_dsl::market_negotiation_event)
                    .values(instance)
                    .execute(conn)?,
            )
        })
        .await?;
        Ok(())
    }
}
