use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InvoiceEvent {
    #[serde(rename = "invoiceId")]
    pub invoice_id: String,
    #[serde(rename = "timestamp")]
    pub timestamp: String,
    #[serde(rename = "details", skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    #[serde(rename = "eventType")]
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
