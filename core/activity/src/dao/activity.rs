use crate::dao::{FlattenInnerOption, Result};
use crate::db::ConnType;
use crate::timeout::Interval;
use diesel::prelude::*;
use futures::task::{Context, Poll};
use futures::Future;
use serde_json;
use std::pin::Pin;
use ya_model::activity::{ActivityState, ActivityUsage, State};

pub struct ActivityDao<'c> {
    conn: &'c ConnType,
}

impl<'c> ActivityDao<'c> {
    pub fn new(conn: &'c ConnType) -> Self {
        Self { conn }
    }
}

impl<'c> ActivityDao<'c> {
    pub fn create(
        &self,
        activity_id: &str,
        agreement_id: &str,
        state: Option<ActivityState>,
        usage: Option<ActivityUsage>,
    ) -> Result<()> {
        use crate::db::schema::activities::dsl;

        let state = state.map(|s| serde_json::to_string(&s).unwrap());
        let usage = usage.map(|u| serde_json::to_string(&u).unwrap());

        diesel::insert_into(dsl::activities)
            .values((
                dsl::id.eq(activity_id),
                dsl::agreement_id.eq(agreement_id),
                dsl::state.eq(state),
                dsl::usage.eq(usage),
            ))
            .execute(self.conn)
            .map(|_| ())
    }

    pub fn get_agreement_id(&self, activity_id: &str) -> Result<String> {
        use crate::db::schema::activities::dsl;

        dsl::activities
            .select(dsl::agreement_id)
            .filter(dsl::id.eq(activity_id))
            .first(self.conn)
    }

    pub fn get_state(&self, activity_id: &str) -> Result<ActivityState> {
        use crate::db::schema::activities::dsl;

        dsl::activities
            .select(dsl::state)
            .filter(dsl::id.eq(activity_id))
            .first::<Option<String>>(self.conn)
            .map(|opt| opt.and_then(|json| serde_json::from_str::<ActivityState>(&json).ok()))
            .flatten_inner_option()
    }

    pub fn get_state_fut<'l>(
        &'l self,
        activity_id: &'l str,
        state: Option<State>,
    ) -> StateFuture<'l, '_> {
        StateFuture::new(self, activity_id, state)
    }

    pub fn set_state(&self, activity_id: &str, activity_state: &ActivityState) -> Result<()> {
        use crate::db::schema::activities::dsl;

        let state = Some(serde_json::to_string(&activity_state).unwrap());

        self.conn.transaction(|| {
            let updated = diesel::update(dsl::activities.filter(dsl::id.eq(activity_id)))
                .set(dsl::state.eq(state))
                .execute(self.conn)?;

            match updated {
                0 => Err(diesel::result::Error::NotFound),
                1 => Ok(()),
                _ => Err(diesel::result::Error::RollbackTransaction),
            }
        })
    }

    pub fn get_usage(&self, activity_id: &str) -> Result<ActivityUsage> {
        use crate::db::schema::activities::dsl;

        dsl::activities
            .filter(dsl::id.eq(activity_id.to_string()))
            .select(dsl::usage)
            .first::<Option<String>>(self.conn)
            .map(|opt| opt.and_then(|json| serde_json::from_str::<ActivityUsage>(&json).ok()))
            .flatten_inner_option()
    }

    pub fn set_usage(&self, activity_id: &str, activity_usage: &ActivityUsage) -> Result<()> {
        use crate::db::schema::activities::dsl;

        let usage = Some(serde_json::to_string(&activity_usage).unwrap());

        self.conn.transaction(|| {
            let updated = diesel::update(dsl::activities.filter(dsl::id.eq(activity_id)))
                .set(dsl::usage.eq(usage))
                .execute(self.conn)?;

            match updated {
                0 => Err(diesel::result::Error::NotFound),
                1 => Ok(()),
                _ => Err(diesel::result::Error::RollbackTransaction),
            }
        })
    }
}

pub struct StateFuture<'l, 'c: 'l> {
    dao: &'l ActivityDao<'c>,
    activity_id: &'l str,
    state: Option<State>,
    interval: Interval,
}

impl<'l, 'c: 'l> StateFuture<'l, 'c> {
    fn new(dao: &'l ActivityDao<'c>, activity_id: &'l str, state: Option<State>) -> Self {
        let interval = Interval::new(500);
        Self {
            dao,
            activity_id,
            state,
            interval,
        }
    }
}

impl<'l, 'c: 'l> Future for StateFuture<'l, 'c> {
    type Output = ActivityState;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.interval.check() {
            if let Ok(state) = self.dao.get_state(&self.activity_id) {
                if self.state.is_none() || self.state.unwrap() == state.state {
                    return Poll::Ready(state);
                }
            }
        }

        ctx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl<'l, 'c: 'l> Unpin for StateFuture<'l, 'c> {}
