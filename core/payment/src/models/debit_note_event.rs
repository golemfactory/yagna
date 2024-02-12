use crate::error::{DbError, DbResult};
use crate::schema::{pay_debit_note_event, pay_debit_note_event_read};
use chrono::{NaiveDateTime, TimeZone, Utc};
use std::convert::TryFrom;
use ya_client_model::payment::{DebitNoteEvent, DebitNoteEventType};
use ya_client_model::NodeId;
use ya_persistence::types::{AdaptTimestamp, Role, TimestampAdapter};

#[derive(Debug, Identifiable, Insertable)]
#[table_name = "pay_debit_note_event"]
#[primary_key(debit_note_id, event_type)]
pub struct WriteObj {
    pub debit_note_id: String,
    pub owner_id: NodeId,
    pub event_type: String,
    pub details: Option<String>,
    pub timestamp: TimestampAdapter,
}

impl WriteObj {
    pub fn new(
        debit_note_id: String,
        owner_id: NodeId,
        event_type: DebitNoteEventType,
    ) -> DbResult<Self> {
        let details = match event_type.details() {
            Some(details) => Some(serde_json::to_string(&details)?),
            None => None,
        };

        Ok(Self {
            debit_note_id,
            owner_id,
            event_type: event_type.discriminant().to_owned(),
            details,
            timestamp: Utc::now().adapt(),
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
        let details = match &event.details {
            Some(text) => Some(
                serde_json::from_str::<serde_json::Value>(text)
                    .map_err(|e| DbError::Integrity(e.to_string()))?,
            ),
            None => None,
        };
        let event_type =
            DebitNoteEventType::from_discriminant_and_details(&event.event_type, details.clone())
                .ok_or_else(|| {
                DbError::Integrity(format!(
                    "event = {}, details = {:#?} is not valid DebitNoteEventType",
                    &event.event_type, details
                ))
            })?;

        Ok(Self {
            debit_note_id: event.debit_note_id,
            event_date: Utc.from_utc_datetime(&event.timestamp),
            event_type,
        })
    }
}
