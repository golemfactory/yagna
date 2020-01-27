use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InvoiceEvent {
    #[serde(rename = "invoiceId", skip_serializing_if = "Option::is_none")]
    pub invoice_id: Option<String>,
    #[serde(rename = "timestamp", skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(rename = "eventType", skip_serializing_if = "Option::is_none")]
    pub event_type: Option<crate::payment::EventType>,
}

impl InvoiceEvent {
    pub fn new() -> InvoiceEvent {
        InvoiceEvent {
            invoice_id: None,
            timestamp: None,
            event_type: None,
        }
    }
}
