use crate::dao::{NotFoundAsOption, Result};
use chrono::Utc;
use diesel::prelude::*;
use diesel::sql_types::Timestamp;
use std::cmp::min;
use std::time::Duration;
use tokio::time::delay_for;
use ya_persistence::executor::ConnType;
use ya_persistence::schema;

pub const MAX_EVENTS: u32 = 100;

#[derive(Queryable, Debug)]
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

        let now = Utc::now().naive_utc();

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
                        .filter(dsl_type::name.eq(event_type))
                        .limit(1),
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

        let limit = match max_count {
            Some(val) => min(MAX_EVENTS, val),
            None => MAX_EVENTS,
        };

        log::debug!("starting db query");
        self.conn.transaction(|| {
            let results: Vec<Event> = dsl_event::activity_event
                .inner_join(schema::activity_event_type::table)
                .inner_join(schema::activity::table)
                .select((
                    dsl_event::id,
                    dsl_type::name,
                    dsl::natural_id,
                    dsl::agreement_id,
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

    pub async fn get_events_fut(&self, max_count: Option<u32>) -> Result<Vec<Event>> {
        let duration = Duration::from_millis(750);

        loop {
            let result = self.get_events(max_count).not_found_as_option()?;
            if let Some(events) = result {
                if events.len() > 0 {
                    return Ok(events);
                }
            }

            delay_for(duration).await;
        }
    }
}
