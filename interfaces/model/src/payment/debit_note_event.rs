use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DebitNoteEvent {
    #[serde(rename = "debitNoteId", skip_serializing_if = "Option::is_none")]
    pub debit_note_id: Option<String>,
    #[serde(rename = "timestamp", skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(rename = "eventType", skip_serializing_if = "Option::is_none")]
    pub event_type: Option<crate::payment::EventType>,
}

impl DebitNoteEvent {
    pub fn new() -> DebitNoteEvent {
        DebitNoteEvent {
            debit_note_id: None,
            timestamp: None,
            event_type: None,
        }
    }
}
