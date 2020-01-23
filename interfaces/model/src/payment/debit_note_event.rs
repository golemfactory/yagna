use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DebitNoteEvent {
    #[serde(rename = "debitNoteId")]
    pub debit_note_id: String,
    #[serde(rename = "timestamp")]
    pub timestamp: String,
    #[serde(rename = "details", skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    #[serde(rename = "eventType")]
    pub event_type: crate::payment::EventType,
}

impl DebitNoteEvent {
    pub fn new(
        debit_note_id: String,
        timestamp: String,
        event_type: crate::payment::EventType,
    ) -> DebitNoteEvent {
        DebitNoteEvent {
            debit_note_id,
            timestamp,
            details: None,
            event_type,
        }
    }
}
