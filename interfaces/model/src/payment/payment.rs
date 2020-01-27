use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Payment {
    #[serde(rename = "paymentId")]
    pub payment_id: String,
    #[serde(rename = "amount")]
    pub amount: i32,
    #[serde(rename = "timestamp")]
    pub timestamp: String,
    #[serde(rename = "allocationId", skip_serializing_if = "Option::is_none")]
    pub allocation_id: Option<String>,
    #[serde(rename = "debitNoteIds", skip_serializing_if = "Option::is_none")]
    pub debit_note_ids: Option<Vec<String>>,
    #[serde(rename = "invoiceIds", skip_serializing_if = "Option::is_none")]
    pub invoice_ids: Option<Vec<String>>,
    #[serde(rename = "details")]
    pub details: serde_json::Value,
}

impl Payment {
    pub fn new(
        payment_id: String,
        amount: i32,
        timestamp: String,
        details: serde_json::Value,
    ) -> Payment {
        Payment {
            payment_id,
            amount,
            timestamp,
            allocation_id: None,
            debit_note_ids: None,
            invoice_ids: None,
            details,
        }
    }
}
