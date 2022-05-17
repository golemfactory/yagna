use crate::error::{DbError, DbResult};
use crate::schema::{pay_debit_note_event, pay_debit_note_event_read};
use crate::utils::{json_from_str, json_to_string};
use chrono::{NaiveDateTime, TimeZone, Utc};
use serde::Serialize;
use std::convert::TryFrom;
use ya_client_model::payment::{DebitNoteEvent, DebitNoteEventType};
use ya_client_model::NodeId;
use ya_persistence::types::Role;

#[derive(Debug, Identifiable, Insertable)]
#[table_name = "pay_debit_note_event"]
#[primary_key(debit_note_id, event_type)]
pub struct WriteObj {
    pub debit_note_id: String,
    pub owner_id: NodeId,
    pub event_type: String,
    pub details: Option<String>,
}

impl WriteObj {
    pub fn new<T: Serialize>(
        debit_note_id: String,
        owner_id: NodeId,
        event_type: DebitNoteEventType,
        details: Option<T>,
    ) -> DbResult<Self> {
        let details = match details {
            Some(details) => Some(json_to_string(&details)?),
            None => None,
        };
        Ok(Self {
            debit_note_id,
            owner_id,
            event_type: event_type.to_string(),
            details,
        })
    }
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "pay_debit_note_event_read"]
#[primary_key(debit_note_id, event_type)]
pub struct ReadObj {
    pub role: Role,
    pub debit_note_id: String,
    pub owner_id: NodeId,
    pub event_type: String,
    pub timestamp: NaiveDateTime,
    pub details: Option<String>,
    pub app_session_id: Option<String>,
}

impl TryFrom<ReadObj> for DebitNoteEvent {
    type Error = DbError;

    fn try_from(event: ReadObj) -> DbResult<Self> {
        let event_type = event.event_type.parse().map_err(|e| {
            DbError::Integrity(format!(
                "DebitNoteEvent type `{}` parsing failed: {}",
                &event.event_type, e
            ))
        })?;
        // TODO Attach details when event_type=REJECTED
        let _details = match event.details {
            Some(s) => Some(json_from_str(&s)?),
            None => None,
        };
        Ok(Self {
            debit_note_id: event.debit_note_id,
            event_date: Utc.from_utc_datetime(&event.timestamp),
            event_type,
        })
    }
}
