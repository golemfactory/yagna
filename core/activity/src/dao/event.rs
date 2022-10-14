use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use diesel::prelude::*;
use diesel::sql_types::{Integer, Nullable, Text, Timestamp};
use std::time::Duration;
use tokio::time::sleep;

use ya_client_model::activity::ProviderEvent;
use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};

use crate::dao::Result;
use crate::db::{models::ActivityEventType, schema};
use ya_client_model::activity::provider_event::ProviderEventType;
use ya_client_model::NodeId;
use ya_persistence::types::AdaptTimestamp;

pub const MAX_EVENTS: i64 = 100;

#[derive(Queryable, Debug)]
pub struct Event {
    pub id: i32,
    pub event_date: NaiveDateTime,
    pub event_type: ActivityEventType,
    pub activity_natural_id: String,
    pub agreement_natural_id: String,
    pub requestor_pub_key: Option<Vec<u8>>,
}

impl From<Event> for ProviderEvent {
    fn from(value: Event) -> Self {
        let event_type = match value.event_type {
            ActivityEventType::CreateActivity => ProviderEventType::CreateActivity {
                requestor_pub_key: value.requestor_pub_key.map(hex::encode),
            },
            ActivityEventType::DestroyActivity => ProviderEventType::DestroyActivity {},
        };

        ProviderEvent {
            activity_id: value.activity_natural_id,
            agreement_id: value.agreement_natural_id,
            event_type,
            event_date: Utc.from_utc_datetime(&value.event_date),
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
        identity_id: &NodeId,
        event_type: ActivityEventType,
        requestor_pub_key: Option<Vec<u8>>,
        app_session_id: &Option<String>,
    ) -> Result<i32> {
        use schema::activity::dsl;
        use schema::activity_event::dsl as dsl_event;

        log::trace!("creating event_type: {:?}", event_type);

        let app_session_id = app_session_id.to_owned();
        let activity_id = activity_id.to_owned();
        let identity_id = identity_id.to_owned();

        do_with_transaction(self.pool, move |conn| {
            let now = Utc::now().adapt();
            diesel::insert_into(dsl_event::activity_event)
                .values(
                    dsl::activity
                        .select((
                            dsl::id,
                            identity_id.into_sql::<Text>(),
                            now.into_sql::<Timestamp>(),
                            event_type.into_sql::<Integer>(),
                            requestor_pub_key.into_sql(),
                            app_session_id.into_sql::<Nullable<Text>>(),
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
                    dsl_event::app_session_id,
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
        identity_id: &NodeId,
        app_session_id: &Option<String>,
        after_timestamp: DateTime<Utc>,
        max_events: Option<u32>,
    ) -> Result<Option<Vec<ProviderEvent>>> {
        use schema::activity::dsl;
        use schema::activity_event::dsl as dsl_event;

        let identity_id = identity_id.to_string();
        let app_session_id = app_session_id.to_owned();
        let limit = match max_events {
            Some(val) => MAX_EVENTS.min(val as i64),
            None => MAX_EVENTS,
        };

        log::trace!("get_events: starting db query");
        readonly_transaction(self.pool, move |conn| {
            let mut query = dsl_event::activity_event
                .inner_join(schema::activity::table)
                .filter(dsl_event::identity_id.eq(identity_id))
                .select((
                    dsl_event::id,
                    dsl_event::event_date,
                    dsl_event::event_type_id,
                    dsl::natural_id,
                    dsl::agreement_id,
                    dsl_event::requestor_pub_key,
                ))
                .filter(dsl_event::event_date.gt(after_timestamp.adapt()))
                .into_boxed();

            if let Some(app_sid) = app_session_id {
                query = query.filter(dsl_event::app_session_id.eq(app_sid));
            }

            let results: Option<Vec<Event>> = query
                .order(dsl_event::event_date.asc())
                .limit(limit)
                .load::<Event>(conn)
                .optional()?;

            Ok(results.map(|r| r.into_iter().map(ProviderEvent::from).collect()))
        })
        .await
    }

    pub async fn get_events_wait(
        &self,
        identity_id: &NodeId,
        app_session_id: &Option<String>,
        after_timestamp: DateTime<Utc>,
        max_events: Option<u32>,
    ) -> Result<Vec<ProviderEvent>> {
        let duration = Duration::from_millis(750);

        loop {
            if let Some(events) = self
                .get_events(identity_id, app_session_id, after_timestamp, max_events)
                .await?
            {
                if !events.is_empty() {
                    return Ok(events);
                }
            }
            sleep(duration).await;
        }
    }
}
