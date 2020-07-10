use chrono::{NaiveDateTime, Utc};
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};

use ya_persistence::executor::ConnType;
use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};

use crate::db::models::{Demand, SubscriptionId};
use crate::db::schema::market_demand::dsl;
use crate::db::DbResult;

#[allow(unused)]
pub struct DemandDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for DemandDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

/// Returns state of Demand in database.
pub enum DemandState {
    Active(Demand),
    Expired(Option<Demand>),
    NotFound,
}

impl<'c> DemandDao<'c> {
    pub async fn select(&self, subscription_id: &SubscriptionId) -> DbResult<Option<Demand>> {
        let subscription_id = subscription_id.clone();
        let now = Utc::now().naive_utc();

        readonly_transaction(self.pool, move |conn| {
            Ok(dsl::market_demand
                .filter(dsl::id.eq(&subscription_id))
                .filter(dsl::expiration_ts.ge(now))
                .first(conn)
                .optional()?)
        })
        .await
    }

    pub async fn get_demands_before(
        &self,
        insertion_ts: NaiveDateTime,
        validation_ts: NaiveDateTime,
    ) -> DbResult<Vec<Demand>> {
        let now = Utc::now().naive_utc();
        readonly_transaction(self.pool, move |conn| {
            Ok(dsl::market_demand
                // we querying less then here and less equal in Offers
                // not to duplicate pair subscribed at the very same moment
                .filter(dsl::insertion_ts.lt(insertion_ts))
                .filter(dsl::expiration_ts.ge(validation_ts))
                .order_by(dsl::creation_ts.asc())
                .load::<Demand>(conn)?)
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

pub(super) fn demand_status(
    conn: &ConnType,
    subscription_id: &SubscriptionId,
) -> DbResult<DemandState> {
    let demand: Option<Demand> = dsl::market_demand
        .filter(dsl::id.eq(&subscription_id))
        .first(conn)
        .optional()?;

    match demand {
        Some(demand) => match demand.expiration_ts > Utc::now().naive_utc() {
            true => Ok(DemandState::Active(demand)),
            false => Ok(DemandState::Expired(Some(demand))),
        },
        None => Ok(DemandState::NotFound),
    }
}
