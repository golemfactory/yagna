use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebitNote {
    pub debit_note_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_debit_note_id: Option<String>,
    pub timestamp: String,
    pub agreement_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_id: Option<String>,
    pub total_amount_due: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_counter_vector: Option<serde_json::Value>,
    pub credit_account_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_due_date: Option<String>,
    pub status: crate::payment::InvoiceStatus,
}

impl DebitNote {
    pub fn new(
        debit_note_id: String,
        timestamp: String,
        agreement_id: String,
        total_amount_due: i32,
        credit_account_id: String,
        status: crate::payment::InvoiceStatus,
    ) -> DebitNote {
        DebitNote {
            debit_note_id,
            previous_debit_note_id: None,
            timestamp,
            agreement_id,
            activity_id: None,
            total_amount_due,
            usage_counter_vector: None,
            credit_account_id,
            payment_platform: None,
            payment_due_date: None,
            status,
        }
    }
}
