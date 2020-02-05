use crate::dao::{DaoError, NotFoundAsOption, Result};
use chrono::Utc;
use diesel::prelude::*;
use diesel::sql_types::{Integer, Timestamp};
use std::cmp::min;
use std::time::Duration;
use tokio::time::delay_for;
use ya_persistence::executor::{do_with_connection, do_with_transaction, AsDao, PoolType};
use ya_persistence::models::ActivityEventType;
use ya_persistence::schema;

pub const MAX_EVENTS: u32 = 100;

#[derive(Queryable, Debug)]
pub struct Event {
    pub id: i32,
    pub event_type: ActivityEventType,
    pub activity_natural_id: String,
    pub agreement_natural_id: String,
}

pub struct EventDao<'c> {
    pool: &'c PoolType,
}

impl<'a> AsDao<'a> for EventDao<'a> {
    fn as_dao(pool: &'a PoolType) -> Self {
        EventDao { pool }
    }
}

impl<'c> EventDao<'c> {
    pub async fn create(&self, activity_id: &str, event_type: ActivityEventType) -> Result<()> {
        use schema::activity::dsl;
        use schema::activity_event::dsl as dsl_event;

        let now = Utc::now().naive_utc();
        log::trace!("creating event_type: {:?}", event_type);

        let activity_id = activity_id.to_owned();
        do_with_connection(self.pool, move |conn| {
            {
                diesel::insert_into(dsl_event::activity_event)
                    .values(
                        dsl::activity
                            .select((
                                dsl::id,
                                now.into_sql::<Timestamp>(),
                                event_type.into_sql::<Integer>(),
                            ))
                            .filter(dsl::natural_id.eq(activity_id))
                            .limit(1),
                    )
                    .into_columns((
                        dsl_event::activity_id,
                        dsl_event::event_date,
                        dsl_event::event_type_id,
                    ))
                    .execute(conn)
                    .map(|_| ())
            }
            .map_err(DaoError::from)
        })
        .await
    }

    pub async fn get_events(&self, max_count: Option<u32>) -> Result<Vec<Event>> {
        use schema::activity::dsl;
        use schema::activity_event::dsl as dsl_event;

        let limit = match max_count {
            Some(val) => min(MAX_EVENTS, val),
            None => MAX_EVENTS,
        };

        log::trace!("get_events: starting db query");
        do_with_transaction(self.pool, move |conn| {
            let results: Vec<Event> = dsl_event::activity_event
                .inner_join(schema::activity::table)
                .select((
                    dsl_event::id,
                    dsl_event::event_type_id,
                    dsl::natural_id,
                    dsl::agreement_id,
                ))
                .order(dsl_event::event_date.asc())
                .limit(limit as i64)
                .load::<Event>(conn)?;

            let ids = results.iter().map(|event| event.id).collect::<Vec<_>>();
            if !ids.is_empty() {
                diesel::delete(dsl_event::activity_event.filter(dsl_event::id.eq_any(ids)))
                    .execute(conn)?;
            }
            Ok(results)
        })
        .await
    }

    pub async fn get_events_fut(&self, max_count: Option<u32>) -> Result<Vec<Event>> {
        let duration = Duration::from_millis(750);

        loop {
            let result = self.get_events(max_count).await.not_found_as_option()?;
            if let Some(events) = result {
                if events.len() > 0 {
                    return Ok(events);
                }
            }

            delay_for(duration).await;
        }
    }
}
