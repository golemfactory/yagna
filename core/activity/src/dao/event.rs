use crate::dao::Result;
use crate::db::ConnType;
use crate::timeout::Interval;
use chrono::Utc;
use diesel::prelude::*;
use futures::future::Future;
use futures::task::{Context, Poll};
use std::cmp::min;
use std::pin::Pin;
use ya_model::activity::ProviderEvent;

pub const MAX_EVENTS: u32 = 100;

pub struct EventDao<'c> {
    conn: &'c ConnType,
}

impl<'c> EventDao<'c> {
    pub fn new(conn: &'c ConnType) -> Self {
        Self { conn }
    }
}

impl<'c> EventDao<'c> {
    pub fn create(&self, event: &ProviderEvent) -> Result<()> {
        use crate::db::schema::events::dsl;

        diesel::insert_into(dsl::events)
            .values((
                dsl::created_at.eq(Utc::now().naive_utc()),
                dsl::data.eq(serde_json::to_string(&event).unwrap()),
            ))
            .execute(self.conn)
            .map(|_| ())
    }

    pub fn get_events(&self, max_count: Option<u32>) -> Result<Vec<ProviderEvent>> {
        use crate::db::schema::events::dsl;

        let limit = match max_count {
            Some(val) => min(MAX_EVENTS, val),
            None => MAX_EVENTS,
        };

        self.conn.transaction::<_, _, _>(|| {
            let mut ids = Vec::new();
            let mut events = Vec::new();

            let results = dsl::events
                .select((dsl::id, dsl::data))
                .order(dsl::created_at.asc())
                .limit(limit as i64)
                .load::<(i32, String)>(self.conn)?;

            results.iter().for_each(|(id, data)| {
                ids.push(id);
                events.push(serde_json::from_str::<ProviderEvent>(&data).unwrap());
            });

            diesel::delete(dsl::events.filter(dsl::id.eq_any(ids))).execute(self.conn)?;

            Ok(events)
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
    type Output = Vec<ProviderEvent>;

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
