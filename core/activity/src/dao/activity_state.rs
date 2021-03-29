use chrono::Utc;
use diesel::expression::dsl::exists;
use diesel::prelude::*;
use diesel::QueryableByName;
use serde_json;
use std::{convert::TryInto, time::Duration};
use tokio::time::delay_for;

use ya_client_model::activity::activity_state::{ActivityState, StatePair};
use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};

use crate::dao::{DaoError, Result};
use crate::db::{models::ActivityState as DbActivityState, schema};
use std::collections::BTreeMap;
use ya_client_model::activity::State;

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

        readonly_transaction(self.pool, move |conn| {
            Ok(dsl::activity
                .inner_join(schema::activity_state::table)
                .select(schema::activity_state::all_columns)
                .filter(dsl::natural_id.eq(&activity_id))
                .first::<DbActivityState>(conn)
                .map_err(|e| match e {
                    diesel::NotFound => {
                        DaoError::NotFound(format!("activity state: {}", activity_id))
                    }
                    e => e.into(),
                })?
                .try_into()?)
        })
        .await
    }

    pub async fn stats(&self) -> Result<BTreeMap<State, u64>> {
        readonly_transaction(self.pool, move |conn| {
            use diesel::sql_types::{Integer, Text};
            #[derive(QueryableByName, PartialEq, Debug)]
            struct StatRecord {
                #[sql_type = "Text"]
                state: String,
                #[sql_type = "Integer"]
                n: i32,
            }

            let stats: Vec<StatRecord> = diesel::sql_query(
                r#"
            select b.name state, count(a.natural_id) n
            from activity a join activity_state b on (a.state_id = b.id) group by b.name
            "#,
            )
            .load(conn)
            .map_err(DaoError::from)?;
            let mut m = BTreeMap::new();
            for v in stats {
                let pair: StatePair = serde_json::from_str(&v.state)?;
                if pair.1.is_none() {
                    m.insert(pair.0, v.n as u64);
                }
            }
            Ok(m)
        })
        .await
    }

    pub async fn stats_1h(&self) -> Result<BTreeMap<State, u64>> {
        readonly_transaction(self.pool, move |conn| {
            use diesel::sql_types::{Integer, Text};
            #[derive(QueryableByName, PartialEq, Debug)]
            struct StatRecord {
                #[sql_type = "Text"]
                state: String,
                #[sql_type = "Integer"]
                n: i32,
            }

            let stats: Vec<StatRecord> = diesel::sql_query(
                r#"
            select b.name state, count(a.natural_id) n
            from activity a join activity_state b on (a.state_id = b.id)
            where b.updated_date >= datetime('now', '-1 hour')
            group by b.name
            "#,
            )
            .load(conn)
            .map_err(DaoError::from)?;
            let mut m = BTreeMap::new();
            for v in stats {
                let pair: StatePair = serde_json::from_str(&v.state)?;
                if pair.1.is_none() {
                    m.insert(pair.0, v.n as u64);
                }
            }
            Ok(m)
        })
        .await
    }

    pub async fn get_state_wait(
        &self,
        activity_id: &str,
        states: Vec<StatePair>,
    ) -> Result<ActivityState> {
        let duration = Duration::from_millis(750);

        log::debug!("waiting {:?} for activity states: {:?}", duration, states);
        loop {
            let result = self.get(activity_id).await;
            if let Ok(s) = result {
                if states.contains(&s.state) {
                    log::debug!("got requested state: {:?}", s.state);
                    return Ok(s);
                }
                log::debug!("got state: {:?} != {:?}. Waiting...", s.state, states);
            }

            delay_for(duration).await;
        }
    }

    pub async fn set(&self, activity_id: &str, state: ActivityState) -> Result<ActivityState> {
        use schema::activity::dsl;
        use schema::activity_state::dsl as dsl_state;

        let str_state_pair = serde_json::to_string(&state.state)?;
        let now = Utc::now().naive_utc();
        let activity_id = activity_id.to_owned();

        do_with_transaction(self.pool, move |conn| {
            let num_updates = diesel::update(
                dsl_state::activity_state.filter(exists(
                    dsl::activity
                        .filter(dsl::natural_id.eq(activity_id))
                        .filter(dsl::state_id.eq(dsl_state::id)),
                )),
            )
            .set((
                dsl_state::name.eq(&str_state_pair),
                dsl_state::reason.eq(state.reason.clone()),
                dsl_state::error_message.eq(state.error_message.clone()),
                dsl_state::updated_date.eq(now),
            ))
            .execute(conn)?;

            Ok(match num_updates {
                0 => Err(diesel::result::Error::NotFound),
                1 => Ok(state),
                _ => Err(diesel::result::Error::RollbackTransaction),
            }?)
        })
        .await
    }
}
