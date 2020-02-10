use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Payment {
    pub payment_id: String,
    pub amount: i32,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allocation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debit_note_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invoice_ids: Option<Vec<String>>,
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
