use chrono::Utc;

use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};

use crate::db::models::Demand;
use crate::db::schema::market_demand::dsl;
use crate::db::DbResult;
use crate::SubscriptionId;
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};

#[allow(unused)]
pub struct DemandDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for DemandDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> DemandDao<'c> {
    pub async fn select(&self, subscription_id: &SubscriptionId) -> DbResult<Option<Demand>> {
        let subscription_id = subscription_id.clone();
        let now = Utc::now().naive_utc();

        readonly_transaction(self.pool, move |conn| {
            let demand: Option<Demand> = dsl::market_demand
                .filter(dsl::id.eq(&subscription_id))
                .filter(dsl::expiration_ts.ge(now))
                .first(conn)
                .optional()?;
            match demand {
                Some(model_demand) => Ok(Some(model_demand)),
                None => Ok(None),
            }
        })
        .await
    }

    pub async fn insert(&self, demand: &Demand) -> DbResult<()> {
        let demand = demand.clone();
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::market_demand)
                .values(demand)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn delete(&self, subscription_id: &SubscriptionId) -> DbResult<bool> {
        let subscription_id = subscription_id.clone();

        do_with_transaction(self.pool, move |conn| {
            let num_deleted =
                diesel::delete(dsl::market_demand.filter(dsl::id.eq(subscription_id)))
                    .execute(conn)?;
            Ok(num_deleted > 0)
        })
        .await
    }
}
