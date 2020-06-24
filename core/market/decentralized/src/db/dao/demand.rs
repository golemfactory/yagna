use chrono::Utc;

use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};
use ya_persistence::executor::{ConnType, Error};

use crate::db::models::Demand as ModelDemand;
use crate::db::models::SubscriptionId;
use crate::db::schema::market_demand::dsl;
use crate::db::DbResult;
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

/// Returns state of Demand in database.
pub enum DemandState {
    Active(ModelDemand),
    Expired(Option<ModelDemand>),
    NotFound,
}

impl<'c> DemandDao<'c> {
    pub async fn get_demand(
        &self,
        subscription_id: &SubscriptionId,
    ) -> DbResult<Option<ModelDemand>> {
        let subscription_id = subscription_id.clone();
        let now = Utc::now().naive_utc();

        readonly_transaction(self.pool, move |conn| {
            let demand: Option<ModelDemand> = dsl::market_demand
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

    pub async fn create_demand(&self, demand: &ModelDemand) -> DbResult<()> {
        let demand = demand.clone();
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::market_demand)
                .values(demand)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn remove_demand(&self, subscription_id: &SubscriptionId) -> DbResult<bool> {
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
    let demand: Option<ModelDemand> = dsl::market_demand
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
