use chrono::Utc;
use thiserror::Error;

use ya_persistence::executor::Error as DbError;
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};

use crate::db::models::Demand as ModelDemand;
use crate::db::models::Offer as ModelOffer;
use crate::db::models::Proposal;
use crate::db::schema::market_negotiation::dsl as dsl_negotiation;
use crate::db::schema::market_proposal::dsl;
use crate::db::DbResult;
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};

pub struct ProposalDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for ProposalDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> ProposalDao<'c> {
    pub async fn new_initial_proposal(
        &self,
        demand: ModelDemand,
        offer: ModelOffer,
    ) -> DbResult<Proposal> {
        do_with_transaction(self.pool, move |conn| {
            let (proposal, negotiation) = Proposal::new_initial(demand, offer);
            diesel::insert_into(dsl_negotiation::market_negotiation)
                .values(negotiation)
                .execute(conn)?;

            diesel::insert_into(dsl::market_proposal)
                .values(&proposal)
                .execute(conn)?;
            Ok(proposal)
        })
        .await
    }
}
