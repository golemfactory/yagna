use crate::schema::pay_debit_note_event;
use chrono::{NaiveDateTime, TimeZone, Utc};
use serde::Serialize;
use std::convert::TryInto;
use ya_client_model::payment::{DebitNoteEvent, EventType};
use ya_core_model::ethaddr::NodeId;

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
        event_type: EventType,
        details: Option<T>,
    ) -> Self {
        Self {
            debit_note_id,
            owner_id,
            event_type: event_type.into(),
            details: details.map(|s| serde_json::to_string(&s).unwrap()),
        }
    }
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "pay_debit_note_event"]
#[primary_key(debit_note_id, event_type)]
pub struct ReadObj {
    pub debit_note_id: String,
    pub owner_id: NodeId,
    pub event_type: String,
    pub timestamp: NaiveDateTime,
    pub details: Option<String>,
}

impl From<ReadObj> for DebitNoteEvent {
    fn from(event: ReadObj) -> Self {
        Self {
            debit_note_id: event.debit_note_id,
            timestamp: Utc.from_utc_datetime(&event.timestamp),
            details: event.details.map(|s| serde_json::from_str(&s).unwrap()),
            event_type: event.event_type.try_into().unwrap(),
        }
    }
}
