use crate::dao::Result;
use crate::timeout::Interval;
use chrono::Local;
use diesel::expression::dsl::exists;
use diesel::prelude::*;
use futures::task::{Context, Poll};
use futures::Future;
use serde_json;
use std::pin::Pin;
use ya_model::activity::State;
use ya_persistence::executor::ConnType;
use ya_persistence::models::ActivityState;
use ya_persistence::schema;

pub struct ActivityStateDao<'c> {
    conn: &'c ConnType,
}

impl<'c> ActivityStateDao<'c> {
    pub fn new(conn: &'c ConnType) -> Self {
        Self { conn }
    }
}

impl<'c> ActivityStateDao<'c> {
    pub fn get(&self, activity_id: &str) -> Result<ActivityState> {
        use schema::activity::dsl;

        self.conn.transaction(|| {
            let state: ActivityState = dsl::activity
                .inner_join(schema::activity_state::table)
                .select(schema::activity_state::all_columns)
                .filter(dsl::natural_id.eq(activity_id))
                .first(self.conn)?;

            Ok(state)
        })
    }

    pub fn get_future<'l>(
        &'l self,
        activity_id: &'l str,
        state: Option<State>,
    ) -> StateFuture<'l, '_> {
        StateFuture::new(self, activity_id, state)
    }

    pub fn set(
        &self,
        activity_id: &str,
        state: State,
        reason: Option<String>,
        error_message: Option<String>,
    ) -> Result<()> {
        use schema::activity::dsl;
        use schema::activity_state::dsl as dsl_state;

        let state = serde_json::to_string(&state).unwrap();
        let now = Local::now().naive_local();

        self.conn.transaction(|| {
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
            .execute(self.conn)?;

            match num_updates {
                0 => Err(diesel::result::Error::NotFound),
                1 => Ok(()),
                _ => Err(diesel::result::Error::RollbackTransaction),
            }
        })
    }
}

pub struct StateFuture<'l, 'c> {
    dao: &'l ActivityStateDao<'c>,
    activity_id: &'l str,
    state: Option<String>,
    interval: Interval,
}

impl<'l, 'c: 'l> StateFuture<'l, 'c> {
    fn new(dao: &'l ActivityStateDao<'c>, activity_id: &'l str, state: Option<State>) -> Self {
        let interval = Interval::new(500);
        Self {
            dao,
            activity_id,
            state: state.map(|s| serde_json::to_string(&s).unwrap()),
            interval,
        }
    }
}

impl<'l, 'c: 'l> Future for StateFuture<'l, 'c> {
    type Output = ActivityState;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.interval.check() {
            if let Ok(state) = self.dao.get(&self.activity_id) {
                match &self.state {
                    Some(s) => {
                        if &state.name == s {
                            return Poll::Ready(state);
                        }
                    }
                    None => return Poll::Ready(state),
                }
            }
        }

        ctx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl<'l, 'c: 'l> Unpin for StateFuture<'l, 'c> {}
