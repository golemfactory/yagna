use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceEvent {
    pub invoice_id: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub details: Option<serde_json::Value>,
    pub event_type: crate::payment::EventType,
}

impl InvoiceEvent {
    pub fn new(
        invoice_id: String,
        timestamp: String,
        event_type: crate::payment::EventType,
    ) -> InvoiceEvent {
        InvoiceEvent {
            invoice_id,
            timestamp,
            details: None,
            event_type,
        }
    }
}
