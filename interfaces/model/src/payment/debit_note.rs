use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DebitNote {
    #[serde(rename = "debitNoteId")]
    pub debit_note_id: String,
    #[serde(
        rename = "previousDebitNoteId",
        skip_serializing_if = "Option::is_none"
    )]
    pub previous_debit_note_id: Option<String>,
    #[serde(rename = "timestamp")]
    pub timestamp: String,
    #[serde(rename = "agreementId")]
    pub agreement_id: String,
    #[serde(rename = "activityId", skip_serializing_if = "Option::is_none")]
    pub activity_id: Option<String>,
    #[serde(rename = "totalAmountDue")]
    pub total_amount_due: i32,
    #[serde(rename = "usageCounterVector", skip_serializing_if = "Option::is_none")]
    pub usage_counter_vector: Option<serde_json::Value>,
    #[serde(rename = "creditAccountId")]
    pub credit_account_id: String,
    #[serde(rename = "paymentPlatform", skip_serializing_if = "Option::is_none")]
    pub payment_platform: Option<String>,
    #[serde(rename = "paymentDueDate", skip_serializing_if = "Option::is_none")]
    pub payment_due_date: Option<String>,
    #[serde(rename = "status")]
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
