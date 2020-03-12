use crate::dao::{DaoError, Result};
use chrono::Utc;
use diesel::expression::dsl::exists;
use diesel::prelude::*;
use serde_json;
use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};
use ya_persistence::models::ActivityUsage;
use ya_persistence::schema;

pub struct ActivityUsageDao<'c> {
    pool: &'c PoolType,
}

impl<'a> AsDao<'a> for ActivityUsageDao<'a> {
    fn as_dao(pool: &'a PoolType) -> Self {
        ActivityUsageDao { pool }
    }
}

impl<'c> ActivityUsageDao<'c> {
    pub async fn get(&self, activity_id: &str) -> Result<Option<ActivityUsage>> {
        use schema::activity::dsl;
        use schema::activity_usage::dsl as dsl_usage;

        let activity_id = activity_id.to_owned();

        do_with_transaction(self.pool, move |conn| {
            dsl::activity
                .inner_join(dsl_usage::activity_usage)
                .select(schema::activity_usage::all_columns)
                .filter(dsl::natural_id.eq(activity_id))
                .first(conn)
                .optional()
                .map_err(DaoError::from)
        })
        .await
    }

    pub async fn set(&self, activity_id: &str, vector: &Option<Vec<f64>>) -> Result<()> {
        use schema::activity::dsl;
        use schema::activity_usage::dsl as dsl_usage;

        let vector = serde_json::to_string(vector).unwrap();
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
            .execute(conn)
            .map_err(DaoError::from)?;

            Ok(())
        })
        .await
    }
}
