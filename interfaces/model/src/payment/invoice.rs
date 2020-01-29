use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invoice {
    pub invoice_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_debit_note_id: Option<String>,
    pub timestamp: String,
    pub agreement_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_ids: Option<Vec<String>>,
    pub amount: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_counter_vector: Option<serde_json::Value>,
    pub credit_account_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_platform: Option<String>,
    pub payment_due_date: String,
    pub status: crate::payment::InvoiceStatus,
}

impl Invoice {
    pub fn new(
        invoice_id: String,
        timestamp: String,
        agreement_id: String,
        amount: i32,
        credit_account_id: String,
        payment_due_date: String,
        status: crate::payment::InvoiceStatus,
    ) -> Invoice {
        Invoice {
            invoice_id,
            last_debit_note_id: None,
            timestamp,
            agreement_id,
            activity_ids: None,
            amount,
            usage_counter_vector: None,
            credit_account_id,
            payment_platform: None,
            payment_due_date,
            status,
        }
    }
}
