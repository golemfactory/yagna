use chrono::Utc;
use diesel::expression::dsl::exists;
use diesel::prelude::*;

use std::convert::TryInto;

use ya_client_model::activity::ActivityUsage;
use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};

use crate::dao::{DaoError, Result};
use crate::db::{models::ActivityUsage as DbActivityUsage, schema};

pub struct ActivityUsageDao<'c> {
    pool: &'c PoolType,
}

impl<'a> AsDao<'a> for ActivityUsageDao<'a> {
    fn as_dao(pool: &'a PoolType) -> Self {
        ActivityUsageDao { pool }
    }
}

impl<'c> ActivityUsageDao<'c> {
    pub async fn get(&self, activity_id: &str) -> Result<ActivityUsage> {
        use schema::activity::dsl;
        use schema::activity_usage::dsl as dsl_usage;

        let activity_id = activity_id.to_owned();

        do_with_transaction(self.pool, move |conn| {
            Ok(dsl::activity
                .inner_join(dsl_usage::activity_usage)
                .select(schema::activity_usage::all_columns)
                .filter(dsl::natural_id.eq(&activity_id))
                .first::<DbActivityUsage>(conn)
                .map_err(|e| match e {
                    diesel::NotFound => {
                        DaoError::NotFound(format!("activity usage: {}", activity_id))
                    }
                    e => e.into(),
                })?
                .try_into()?)
        })
        .await
    }

    pub async fn set(&self, activity_id: &str, usage: ActivityUsage) -> Result<ActivityUsage> {
        use schema::activity::dsl;
        use schema::activity_usage::dsl as dsl_usage;

        let vector = serde_json::to_string(&usage.current_usage)?;
        let now = Utc::now().naive_utc();

        let activity_id = activity_id.to_owned();

        do_with_transaction(self.pool, move |conn| {
            diesel::update(
                dsl_usage::activity_usage.filter(exists(
                    dsl::activity
                        .filter(dsl::natural_id.eq(activity_id))
                        .filter(dsl::usage_id.eq(dsl_usage::id)),
                )),
            )
            .set((
                dsl_usage::vector_json.eq(&vector),
                dsl_usage::updated_date.eq(now),
            ))
            .execute(conn)?;

            Ok(usage)
        })
        .await
    }
}
