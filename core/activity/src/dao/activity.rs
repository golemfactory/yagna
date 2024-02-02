use chrono::Utc;
use diesel::prelude::*;

use ya_client_model::activity::{State, StatePair};
use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};

use crate::dao::{last_insert_rowid, DaoError, Result};
use crate::db::schema;
use diesel::dsl::exists;

pub struct ActivityDao<'c> {
    pool: &'c PoolType,
}

impl<'a> AsDao<'a> for ActivityDao<'a> {
    fn as_dao(pool: &'a PoolType) -> Self {
        ActivityDao { pool }
    }
}

impl<'c> ActivityDao<'c> {
    pub async fn get_agreement_id(&self, activity_id: &str) -> Result<String> {
        use schema::activity::dsl;

        let activity_id = activity_id.to_owned();

        do_with_transaction(self.pool, "activity_dao_get_agreement_id", move |conn| {
            dsl::activity
                .select(dsl::agreement_id)
                .filter(dsl::natural_id.eq(&activity_id))
                .first(conn)
                .map_err(|e| match e {
                    diesel::NotFound => {
                        DaoError::NotFound(format!("agreement id for activity: {}", activity_id))
                    }
                    e => e.into(),
                })
        })
        .await
    }

    pub async fn create(&self, activity_id: &str, agreement_id: &str) -> Result<()> {
        use schema::activity::dsl;
        use schema::activity_state::dsl as dsl_state;
        use schema::activity_usage::dsl as dsl_usage;

        let reason: Option<String> = None;
        let error_message: Option<String> = None;
        let vector_json: Option<String> = None;
        let state = serde_json::to_string(&StatePair(State::New, None)).unwrap();
        let now = Utc::now().naive_utc();

        let activity_id = activity_id.to_owned();
        let agreement_id = agreement_id.to_owned();

        do_with_transaction(self.pool, "activity_dao_create", move |conn| {
            diesel::insert_into(dsl_state::activity_state)
                .values((
                    dsl_state::name.eq(&state),
                    dsl_state::reason.eq(reason),
                    dsl_state::error_message.eq(error_message),
                    dsl_state::updated_date.eq(now),
                ))
                .execute(conn)?;

            let state_id: i32 = diesel::select(last_insert_rowid).first(conn)?;

            diesel::insert_into(dsl_usage::activity_usage)
                .values((
                    dsl_usage::vector_json.eq(vector_json),
                    dsl_usage::updated_date.eq(now),
                ))
                .execute(conn)?;

            let usage_id: i32 = diesel::select(last_insert_rowid).first(conn)?;

            diesel::insert_into(dsl::activity)
                .values((
                    dsl::natural_id.eq(activity_id),
                    dsl::agreement_id.eq(agreement_id),
                    dsl::state_id.eq(state_id),
                    dsl::usage_id.eq(usage_id),
                ))
                .execute(conn)?;

            Ok(())
        })
        .await
    }

    pub async fn create_if_not_exists(&self, activity_id: &str, agreement_id: &str) -> Result<()> {
        if let Err(e) = self.create(activity_id, agreement_id).await {
            if !self.exists(activity_id, agreement_id).await? {
                return Err(e);
            }
        }
        Ok(())
    }

    async fn exists(&self, activity_id: &str, agreement_id: &str) -> Result<bool> {
        use schema::activity::dsl;

        let activity_id = activity_id.to_owned();
        let agreement_id = agreement_id.to_owned();

        do_with_transaction(self.pool, "activity_dao_exists", move |conn| {
            Ok(diesel::select(exists(
                dsl::activity
                    .filter(dsl::natural_id.eq(activity_id))
                    .filter(dsl::agreement_id.eq(agreement_id)),
            ))
            .get_result(conn)?)
        })
        .await
    }

    pub async fn _get_activity_ids(&self) -> Result<Vec<String>> {
        use schema::activity::dsl;
        do_with_transaction(self.pool, "activity_dao_get_activity_ids", |conn| {
            dsl::activity
                .select(dsl::natural_id)
                .get_results(conn)
                .map_err(|e| e.into())
        })
        .await
    }
}
