use crate::dao::{last_insert_rowid, Result};
use chrono::Utc;
use diesel::expression::exists::exists;
use diesel::prelude::*;
use diesel::sql_types::{Integer, VarChar};
use serde_json;
use ya_model::activity::State;
use ya_persistence::executor::ConnType;
use ya_persistence::schema;

pub struct ActivityDao<'c> {
    conn: &'c ConnType,
}

impl<'c> ActivityDao<'c> {
    pub fn new(conn: &'c ConnType) -> Self {
        Self { conn }
    }
}

impl<'c> ActivityDao<'c> {
    pub fn exists(&self, activity_id: &str) -> Result<bool> {
        use schema::activity::dsl;

        self.conn.transaction(|| {
            diesel::select(exists(
                dsl::activity.filter(dsl::natural_id.eq(activity_id)),
            ))
            .get_result(self.conn)
        })
    }

    pub fn get_agreement_id(&self, activity_id: &str) -> Result<String> {
        use schema::activity::dsl;
        use schema::agreement::dsl as dsl_agreement;

        self.conn.transaction(|| {
            let agreement: String = dsl::activity
                .inner_join(schema::agreement::table)
                .select(dsl_agreement::natural_id)
                .filter(dsl::natural_id.eq(activity_id))
                .first(self.conn)?;

            Ok(agreement)
        })
    }

    pub fn create(&self, activity_id: &str, agreement_id: &str) -> Result<()> {
        use schema::activity::dsl;
        use schema::activity_state::dsl as dsl_state;
        use schema::activity_usage::dsl as dsl_usage;
        use schema::agreement::dsl as dsl_agreement;

        let reason: Option<String> = None;
        let error_message: Option<String> = None;
        let vector_json: Option<String> = None;
        let state = serde_json::to_string(&State::New).unwrap();
        let now = Utc::now().naive_utc();

        self.conn.transaction(|| {
            diesel::insert_into(dsl_state::activity_state)
                .values((
                    dsl_state::name.eq(&state),
                    dsl_state::reason.eq(reason),
                    dsl_state::error_message.eq(error_message),
                    dsl_state::updated_date.eq(now),
                ))
                .execute(self.conn)?;

            let state_id: i64 = diesel::select(last_insert_rowid).first(self.conn)?;

            diesel::insert_into(dsl_usage::activity_usage)
                .values((
                    dsl_usage::vector_json.eq(vector_json),
                    dsl_usage::updated_date.eq(now),
                ))
                .execute(self.conn)?;

            let usage_id: i64 = diesel::select(last_insert_rowid).first(self.conn)?;

            diesel::insert_into(dsl::activity)
                .values(
                    dsl_agreement::agreement
                        .select((
                            activity_id.into_sql::<VarChar>(),
                            dsl_agreement::id,
                            (state_id as i32).into_sql::<Integer>(),
                            (usage_id as i32).into_sql::<Integer>(),
                        ))
                        .filter(dsl_agreement::natural_id.eq(agreement_id))
                        .limit(1),
                )
                .into_columns((
                    dsl::natural_id,
                    dsl::agreement_id,
                    dsl::state_id,
                    dsl::usage_id,
                ))
                .execute(self.conn)
                .map(|_| ())?;

            Ok(())
        })
    }
}
