use chrono::Utc;
use diesel::prelude::*;
use diesel::sql_types::{Integer, Text, Timestamp};
use std::cmp::min;
use std::time::Duration;
use tokio::time::delay_for;

use ya_client_model::activity::ProviderEvent;
use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};

use crate::dao::Result;
use crate::db::{models::ActivityEventType, schema};

pub const MAX_EVENTS: u32 = 100;

#[derive(Queryable, Debug)]
pub struct Event {
    pub id: i32,
    pub event_type: ActivityEventType,
    pub activity_natural_id: String,
    pub agreement_natural_id: String,
    pub requestor_pub_key: Option<Vec<u8>>,
}

impl From<Event> for ProviderEvent {
    fn from(value: Event) -> Self {
        match value.event_type {
            ActivityEventType::CreateActivity => ProviderEvent::CreateActivity {
                activity_id: value.activity_natural_id,
                agreement_id: value.agreement_natural_id,
                requestor_pub_key: value.requestor_pub_key.map(hex::encode),
            },
            ActivityEventType::DestroyActivity => ProviderEvent::DestroyActivity {
                activity_id: value.activity_natural_id,
                agreement_id: value.agreement_natural_id,
            },
        }
    }
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
    pub async fn create(
        &self,
        activity_id: &str,
        identity_id: &str,
        event_type: ActivityEventType,
        requestor_pub_key: Option<Vec<u8>>,
    ) -> Result<i32> {
        use schema::activity::dsl;
        use schema::activity_event::dsl as dsl_event;

        let now = Utc::now().naive_utc();
        log::trace!("creating event_type: {:?}", event_type);

        let activity_id = activity_id.to_owned();
        let identity_id = identity_id.to_owned();

        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl_event::activity_event)
                .values(
                    dsl::activity
                        .select((
                            dsl::id,
                            identity_id.clone().into_sql::<Text>(),
                            now.into_sql::<Timestamp>(),
                            event_type.into_sql::<Integer>(),
                            requestor_pub_key.into_sql(),
                        ))
                        .filter(dsl::natural_id.eq(activity_id))
                        .limit(1),
                )
                .into_columns((
                    dsl_event::activity_id,
                    dsl_event::identity_id,
                    dsl_event::event_date,
                    dsl_event::event_type_id,
                    dsl_event::requestor_pub_key,
                ))
                .execute(conn)?;

            let event_id = diesel::select(super::last_insert_rowid).first(conn)?;
            log::trace!("event inserted: {}", event_id);

            Ok(event_id)
        })
        .await
    }

    pub async fn get_events(
        &self,
        identity_id: &str,
        max_events: Option<u32>,
    ) -> Result<Option<Vec<ProviderEvent>>> {
        use schema::activity::dsl;
        use schema::activity_event::dsl as dsl_event;

        let limit = match max_events {
            Some(val) => min(MAX_EVENTS, val),
            None => MAX_EVENTS,
        };

        log::trace!("get_events: starting db query");
        let identity_id = identity_id.to_owned();

        do_with_transaction(self.pool, move |conn| {
            let results: Option<Vec<Event>> = dsl_event::activity_event
                .inner_join(schema::activity::table)
                .filter(dsl_event::identity_id.eq(identity_id))
                .select((
                    dsl_event::id,
                    dsl_event::event_type_id,
                    dsl::natural_id,
                    dsl::agreement_id,
                    dsl_event::requestor_pub_key,
                ))
                .order(dsl_event::event_date.asc())
                .limit(limit as i64)
                .load::<Event>(conn)
                .optional()?;

            if let Some(r) = &results {
                let ids = r.iter().map(|event| event.id).collect::<Vec<_>>();
                if !ids.is_empty() {
                    diesel::delete(dsl_event::activity_event.filter(dsl_event::id.eq_any(ids)))
                        .execute(conn)?;
                }
            }

            Ok(results.map(|r| r.into_iter().map(ProviderEvent::from).collect()))
        })
        .await
    }

    pub async fn get_events_wait(
        &self,
        identity_id: impl ToString,
        max_events: Option<u32>,
    ) -> Result<Vec<ProviderEvent>> {
        let duration = Duration::from_millis(750);
        let identity_id = identity_id.to_string();

        loop {
            let result = self.get_events(&identity_id, max_events).await?;
            if let Some(events) = result {
                if events.len() > 0 {
                    return Ok(events);
                }
            }

            delay_for(duration).await;
        }
    }
}
