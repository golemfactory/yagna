use chrono::{NaiveDateTime, Utc};
use diesel::expression::dsl::now as sql_now;
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};

use ya_client::model::NodeId;
use ya_persistence::executor::ConnType;
use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};

use crate::db::model::{Demand, SubscriptionId};
use crate::db::schema::market_demand::dsl;
use crate::db::{DbError, DbResult};

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
    // TODO: return DemandState
    pub async fn select(&self, id: &SubscriptionId) -> DbResult<Option<Demand>> {
        let id = id.clone();
        let now = Utc::now().naive_utc();

        readonly_transaction(self.pool, move |conn| {
            Ok(dsl::market_demand
                .filter(dsl::id.eq(&id))
                .filter(dsl::expiration_ts.ge(now))
                .first(conn)
                .optional()?)
        })
        .await
    }

    pub async fn get_demands(
        &self,
        node_id: Option<NodeId>,
        insertion_ts: Option<NaiveDateTime>,
        validation_ts: NaiveDateTime,
    ) -> DbResult<Vec<Demand>> {
        readonly_transaction(self.pool, move |conn| {
            let mut query = dsl::market_demand
                .filter(dsl::expiration_ts.ge(validation_ts))
                .order_by(dsl::creation_ts.asc())
                .into_boxed();

            if let Some(ts) = insertion_ts {
                // we querying less then here and less equal in Offers
                // not to duplicate pair subscribed at the very same moment
                query = query.filter(dsl::insertion_ts.lt(ts));
            };

            if let Some(id) = node_id {
                query = query.filter(dsl::node_id.eq(id));
            };

            Ok(query.load::<Demand>(conn)?)
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

    pub async fn delete(&self, id: &SubscriptionId) -> DbResult<bool> {
        let id = id.clone();

        do_with_transaction(self.pool, move |conn| {
            let num_deleted =
                diesel::delete(dsl::market_demand.filter(dsl::id.eq(id))).execute(conn)?;
            Ok(num_deleted > 0)
        })
        .await
    }

    pub async fn clean(&self) -> DbResult<()> {
        log::debug!("Clean market demands: start");
        let num_deleted = do_with_transaction(self.pool, move |conn| {
            let nd = diesel::delete(dsl::market_demand.filter(dsl::expiration_ts.lt(sql_now)))
                .execute(conn)?;
            Result::<usize, DbError>::Ok(nd)
        })
        .await?;
        if num_deleted > 0 {
            log::info!("Clean market demands: {} cleaned", num_deleted);
        }
        log::debug!("Clean market demands: done");
        Ok(())
    }

    pub async fn demand_state(self, id: &SubscriptionId) -> DbResult<DemandState> {
        let id = id.clone();
        do_with_transaction(self.pool, move |conn| demand_status(conn, &id)).await
    }
}

pub(super) fn demand_status(conn: &ConnType, id: &SubscriptionId) -> DbResult<DemandState> {
    let demand: Option<Demand> = dsl::market_demand
        .filter(dsl::id.eq(&id))
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
