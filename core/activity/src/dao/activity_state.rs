use crate::dao::{DaoError, NotFoundAsOption, Result};
use chrono::Utc;
use diesel::expression::dsl::exists;
use diesel::prelude::*;
use serde_json;
use std::time::Duration;
use tokio::time::delay_for;
use ya_model::activity::State;
use ya_persistence::executor::{do_with_connection, AsDao, PoolType};
use ya_persistence::models::ActivityState;
use ya_persistence::schema;

pub struct ActivityStateDao<'c> {
    pool: &'c PoolType,
}

impl<'a> AsDao<'a> for ActivityStateDao<'a> {
    fn as_dao(pool: &'a PoolType) -> Self {
        ActivityStateDao { pool }
    }
}

impl<'c> ActivityStateDao<'c> {
    pub async fn get(&self, activity_id: &str) -> Result<ActivityState> {
        use schema::activity::dsl;

        log::debug!("getting activity state");
        let activity_id = activity_id.to_owned();
        do_with_connection(self.pool, move |conn| {
            let state: ActivityState = dsl::activity
                .inner_join(schema::activity_state::table)
                .select(schema::activity_state::all_columns)
                .filter(dsl::natural_id.eq(activity_id))
                .first(conn)
                .map_err(DaoError::from)?;

            Ok(state)
        })
        .await
    }

    pub async fn get_future(
        &self,
        activity_id: &str,
        state: Option<State>,
    ) -> Result<ActivityState> {
        let state = state.map(|s| serde_json::to_string(&s).unwrap());
        let duration = Duration::from_millis(750);

        loop {
            let result = self.get(activity_id).await.not_found_as_option()?;
            if let Some(s) = result {
                match &state {
                    Some(state) => {
                        if &s.name == state {
                            return Ok(s);
                        }
                    }
                    None => return Ok(s),
                }
            }

            delay_for(duration).await;
        }
    }

    pub async fn set(
        &self,
        activity_id: &str,
        state: State,
        reason: Option<String>,
        error_message: Option<String>,
    ) -> Result<()> {
        use schema::activity::dsl;
        use schema::activity_state::dsl as dsl_state;

        let state = serde_json::to_string(&state).unwrap();
        let now = Utc::now().naive_utc();

        let activity_id = activity_id.to_owned();
        do_with_connection(self.pool, move |conn| {
            {
                let num_updates = diesel::update(
                    dsl_state::activity_state.filter(exists(
                        dsl::activity
                            .filter(dsl::natural_id.eq(activity_id))
                            .filter(dsl::state_id.eq(dsl_state::id)),
                    )),
                )
                .set((
                    dsl_state::name.eq(&state),
                    dsl_state::reason.eq(reason),
                    dsl_state::error_message.eq(error_message),
                    dsl_state::updated_date.eq(now),
                ))
                .execute(conn)?;

                match num_updates {
                    0 => Err(diesel::result::Error::NotFound),
                    1 => Ok(()),
                    _ => Err(diesel::result::Error::RollbackTransaction),
                }
            }
            .map_err(DaoError::from)
        })
        .await
    }
}
