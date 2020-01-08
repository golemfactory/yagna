use crate::dao::Result;
use crate::timeout::Interval;
use chrono::Local;
use diesel::prelude::*;
use diesel::sql_types::Timestamp;
use futures::future::Future;
use futures::task::{Context, Poll};
use std::cmp::min;
use std::pin::Pin;
use ya_persistence::executor::ConnType;
use ya_persistence::schema;

pub const MAX_EVENTS: u32 = 100;

#[derive(Queryable)]
pub struct Event {
    pub id: i32,
    pub name: String,
    pub activity_natural_id: String,
    pub agreement_natural_id: String,
}

pub struct EventDao<'c> {
    conn: &'c ConnType,
}

impl<'c> EventDao<'c> {
    pub fn new(conn: &'c ConnType) -> Self {
        Self { conn }
    }
}

impl<'c> EventDao<'c> {
    pub fn create(&self, activity_id: &str, event_type: &str) -> Result<()> {
        use schema::activity::dsl;
        use schema::activity_event::dsl as dsl_event;
        use schema::activity_event_type::dsl as dsl_type;

        let now = Local::now().naive_local();

        self.conn.transaction(|| {
            diesel::insert_into(dsl_event::activity_event)
                .values(
                    dsl_event::activity_event
                        .inner_join(schema::activity::table)
                        .inner_join(schema::activity_event_type::table)
                        .select((
                            dsl_event::activity_id,
                            now.into_sql::<Timestamp>(),
                            dsl_type::id,
                        ))
                        .filter(dsl::natural_id.eq(activity_id))
                        .filter(dsl_type::name.eq(event_type)),
                )
                .into_columns((
                    dsl_event::activity_id,
                    dsl_event::event_date,
                    dsl_event::event_type_id,
                ))
                .execute(self.conn)
                .map(|_| ())
        })
    }

    pub fn get_events(&self, max_count: Option<u32>) -> Result<Vec<Event>> {
        use schema::activity::dsl;
        use schema::activity_event::dsl as dsl_event;
        use schema::activity_event_type::dsl as dsl_type;
        use schema::agreement::dsl as dsl_agreement;

        let limit = match max_count {
            Some(val) => min(MAX_EVENTS, val),
            None => MAX_EVENTS,
        };

        self.conn.transaction(|| {
            let results: Vec<Event> = dsl_event::activity_event
                .inner_join(schema::activity_event_type::table)
                .inner_join(schema::activity::table)
                .inner_join(dsl_agreement::agreement.on(dsl_agreement::id.eq(dsl::agreement_id)))
                .select((
                    dsl_event::id,
                    dsl_type::name,
                    dsl::natural_id,
                    dsl_agreement::natural_id,
                ))
                .order(dsl_event::event_date.asc())
                .limit(limit as i64)
                .load::<Event>(self.conn)?;

            let mut ids = Vec::new();
            results.iter().for_each(|event| ids.push(event.id));
            diesel::delete(dsl_event::activity_event.filter(dsl_event::id.eq_any(ids)))
                .execute(self.conn)?;

            Ok(results)
        })
    }

    pub fn get_events_fut(&self, max_count: Option<u32>) -> EventsFuture<'_, '_> {
        EventsFuture::new(self, max_count)
    }
}

pub struct EventsFuture<'d, 'c> {
    dao: &'d EventDao<'c>,
    max_count: Option<u32>,
    interval: Interval,
}

impl<'d, 'c: 'd> EventsFuture<'d, 'c> {
    fn new(dao: &'d EventDao<'c>, max_count: Option<u32>) -> Self {
        let interval = Interval::new(1000);
        Self {
            max_count,
            dao,
            interval,
        }
    }
}

impl<'d, 'c: 'd> Future for EventsFuture<'d, 'c> {
    type Output = Vec<Event>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.interval.check() {
            if let Ok(events) = self.dao.get_events(self.max_count) {
                if events.len() > 0 {
                    return Poll::Ready(events);
                }
            }
        }

        ctx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl<'d, 'c: 'd> Unpin for EventsFuture<'d, 'c> {}
