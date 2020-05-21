use ya_persistence::executor::Error;
use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};

use crate::db::models::Demand as ModelDemand;
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


impl<'c> DemandDao<'c> {
    pub async fn get_demand<Str: AsRef<str>>(
        &self,
        subscription_id: Str,
    ) -> DbResult<Option<ModelDemand>> {
        let subscription_id = subscription_id.as_ref().to_string();
        readonly_transaction(self.pool, move |conn| {
            let demand: Option<ModelDemand> = dsl::market_demand
                .filter(dsl::id.eq(&subscription_id))
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

    pub async fn remove_demand<Str: AsRef<str>>(&self, subscription_id: Str) -> DbResult<bool> {
        let subscription_id = subscription_id.as_ref().to_string();

        do_with_transaction(self.pool, move |conn| {
            let num_deleted = diesel::delete(dsl::market_demand.filter(dsl::id.eq(subscription_id)))
                .execute(conn)?;
            Ok(num_deleted > 0)
        })
            .await
    }
}

