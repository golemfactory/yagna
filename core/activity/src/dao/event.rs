use chrono::{DateTime, NaiveDateTime, TimeZone, Timelike, Utc};
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

/// Compute the `event_date` stamp for a new event row.
///
/// `Utc::now()` is wall-clock and can step backwards (NTP correction, VM
/// suspend/resume). Readers cursor on `event_date > after_timestamp`, so a
/// backwards step would make later-inserted events invisible behind an already
/// delivered watermark and they would never be delivered. Clamping to
/// `prev + 1µs` keeps `event_date` strictly increasing in insertion (rowid)
/// order and free of ties at the microsecond precision the column is stored
/// with (see `TimestampAdapter`).
fn next_event_date(prev: Option<NaiveDateTime>, now: NaiveDateTime) -> NaiveDateTime {
    // Truncate to the microsecond precision that actually reaches the database,
    // otherwise a sub-microsecond remainder would defeat the tie-break below.
    let now = now.with_nanosecond(now.nanosecond() / 1000 * 1000).unwrap();
    match prev {
        Some(prev) if now <= prev => prev + chrono::Duration::microseconds(1),
        _ => now,
    }
}

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

impl EventDao<'_> {
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

        do_with_transaction(self.pool, "event_dao_create", move |conn| {
            // Serialized with every other event insert by the executor's
            // process-wide write lock and sqlite's immediate transaction, so
            // reading the previous maximum here is race-free.
            let prev_event_date: Option<NaiveDateTime> = dsl_event::activity_event
                .select(diesel::dsl::max(dsl_event::event_date))
                .first(conn)?;
            let event_date = next_event_date(prev_event_date, Utc::now().naive_utc()).adapt();

            let inserted = diesel::insert_into(dsl_event::activity_event)
                .values(
                    dsl::activity
                        .select((
                            dsl::id,
                            identity_id.into_sql::<Text>(),
                            event_date.into_sql::<Timestamp>(),
                            event_type.into_sql::<Integer>(),
                            requestor_pub_key.into_sql(),
                            app_session_id.into_sql::<Nullable<Text>>(),
                        ))
                        .filter(dsl::natural_id.eq(&activity_id))
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

            if inserted == 0 {
                // Without this check `last_insert_rowid` below would silently
                // return a stale rowid from this pooled connection.
                return Err(super::DaoError::NotFound(format!(
                    "activity {activity_id}: event not created"
                )));
            }

            let event_id = diesel::select(super::last_insert_rowid()).first(conn)?;
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
        readonly_transaction(self.pool, "event_dao_get_events", move |conn| {
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

            // The id tie-break only matters for rows written before event_date
            // was made strictly increasing: old databases may still contain
            // equal or inverted stamps.
            let results: Option<Vec<Event>> = query
                .order((dsl_event::event_date.asc(), dsl_event::id.asc()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dao::ActivityDao;
    use crate::db::migrations;
    use chrono::NaiveDate;
    use ya_persistence::executor::DbExecutor;

    fn ts(secs: u32, micros: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 8, 12)
            .unwrap()
            .and_hms_micro_opt(12, 0, secs, micros)
            .unwrap()
    }

    #[test]
    fn next_event_date_uses_clock_when_it_moved_forward() {
        assert_eq!(next_event_date(Some(ts(1, 0)), ts(2, 0)), ts(2, 0));
        assert_eq!(next_event_date(None, ts(2, 0)), ts(2, 0));
    }

    #[test]
    fn next_event_date_clamps_backwards_clock_step() {
        assert_eq!(
            next_event_date(Some(ts(15, 611_369)), ts(1, 254_883)),
            ts(15, 611_370)
        );
    }

    #[test]
    fn next_event_date_breaks_same_microsecond_tie() {
        assert_eq!(next_event_date(Some(ts(1, 5)), ts(1, 5)), ts(1, 6));
    }

    #[test]
    fn next_event_date_truncates_to_db_precision() {
        // A sub-microsecond remainder in `now` must not defeat the tie-break:
        // the database stores microseconds only.
        let now = ts(1, 5).with_nanosecond(5_900).unwrap();
        assert_eq!(next_event_date(Some(ts(1, 5)), now), ts(1, 6));
    }

    async fn test_db(name: &str) -> DbExecutor {
        let db = DbExecutor::in_memory(&format!("{name}-{}", uuid::Uuid::new_v4())).unwrap();
        db.apply_migration(migrations::MIGRATIONS).unwrap();
        db
    }

    fn identity() -> NodeId {
        "0xbabe000000000000000000000000000000000000"
            .parse()
            .unwrap()
    }

    #[actix_rt::test]
    async fn event_dates_are_strictly_increasing_and_none_hide_behind_the_cursor() {
        let db = test_db("event-monotonic").await;
        db.as_dao::<ActivityDao>()
            .create_if_not_exists("activity-1", "agreement-1")
            .await
            .unwrap();

        let dao = db.as_dao::<EventDao>();
        for _ in 0..50 {
            dao.create(
                "activity-1",
                &identity(),
                ActivityEventType::CreateActivity,
                None,
                &None,
            )
            .await
            .unwrap();
        }

        let after = Utc.timestamp_opt(0, 0).unwrap();
        let events = dao
            .get_events(&identity(), &None, after, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(events.len(), 50);
        for pair in events.windows(2) {
            assert!(
                pair[0].event_date < pair[1].event_date,
                "event dates must be strictly increasing: {} !< {}",
                pair[0].event_date,
                pair[1].event_date
            );
        }

        // Advancing the cursor to any delivered event's date must expose
        // exactly the events inserted after it.
        let events_after = dao
            .get_events(&identity(), &None, events[46].event_date, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(events_after.len(), 3);
    }

    #[actix_rt::test]
    async fn create_for_unknown_activity_errors_instead_of_stale_rowid() {
        let db = test_db("event-unknown-activity").await;
        let dao = db.as_dao::<EventDao>();
        let result = dao
            .create(
                "no-such-activity",
                &identity(),
                ActivityEventType::CreateActivity,
                None,
                &None,
            )
            .await;
        assert!(matches!(result, Err(crate::dao::DaoError::NotFound(_))));
    }
}
