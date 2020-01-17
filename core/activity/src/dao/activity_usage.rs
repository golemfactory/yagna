use crate::dao::Result;
use chrono::Utc;
use diesel::expression::dsl::exists;
use diesel::prelude::*;
use serde_json;
use ya_persistence::executor::ConnType;
use ya_persistence::models::ActivityUsage;
use ya_persistence::schema;

pub struct ActivityUsageDao<'c> {
    conn: &'c ConnType,
}

impl<'c> ActivityUsageDao<'c> {
    pub fn new(conn: &'c ConnType) -> Self {
        Self { conn }
    }
}

impl<'c> ActivityUsageDao<'c> {
    pub fn get(&self, activity_id: &str) -> Result<ActivityUsage> {
        use schema::activity::dsl;
        use schema::activity_usage::dsl as dsl_usage;

        self.conn.transaction(|| {
            let usage: ActivityUsage = dsl::activity
                .inner_join(dsl_usage::activity_usage)
                .select(schema::activity_usage::all_columns)
                .filter(dsl::natural_id.eq(activity_id))
                .first(self.conn)?;

            Ok(usage)
        })
    }

    pub fn set(&self, activity_id: &str, vector: &Option<Vec<f64>>) -> Result<()> {
        use schema::activity::dsl;
        use schema::activity_usage::dsl as dsl_usage;

        let vector = serde_json::to_string(vector).unwrap();
        let now = Utc::now().naive_utc();

        self.conn.transaction(|| {
            let num_updates = diesel::update(
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
            .execute(self.conn)?;

            match num_updates {
                0 => Err(diesel::result::Error::NotFound),
                1 => Ok(()),
                _ => Err(diesel::result::Error::RollbackTransaction),
            }
        })
    }
}
