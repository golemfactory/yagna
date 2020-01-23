use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Invoice {
    #[serde(rename = "invoiceId")]
    pub invoice_id: String,
    #[serde(rename = "lastDebitNoteId", skip_serializing_if = "Option::is_none")]
    pub last_debit_note_id: Option<String>,
    #[serde(rename = "timestamp")]
    pub timestamp: String,
    #[serde(rename = "agreementId")]
    pub agreement_id: String,
    #[serde(rename = "activityIds", skip_serializing_if = "Option::is_none")]
    pub activity_ids: Option<Vec<String>>,
    #[serde(rename = "amount")]
    pub amount: i32,
    #[serde(rename = "usageCounterVector", skip_serializing_if = "Option::is_none")]
    pub usage_counter_vector: Option<serde_json::Value>,
    #[serde(rename = "creditAccountId")]
    pub credit_account_id: String,
    #[serde(rename = "paymentPlatform", skip_serializing_if = "Option::is_none")]
    pub payment_platform: Option<String>,
    #[serde(rename = "paymentDueDate")]
    pub payment_due_date: String,
    #[serde(rename = "status")]
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
