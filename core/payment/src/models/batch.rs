use std::collections::HashMap;

use bigdecimal::BigDecimal;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use ya_core_model::NodeId;
use ya_persistence::types::BigDecimalField;

use crate::schema::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BatchPaymentObligation {
    Invoice {
        id: String,
        amount: BigDecimal,
        agreement_id: String,
    },
    DebitNote {
        debit_note_id: Option<String>,
        amount: BigDecimal,
        agreement_id: String,
        activity_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BatchPaymentObligationAllocation {
    Invoice {
        id: String,
        amount: BigDecimal,
        agreement_id: String,
        allocation_id: String,
    },
    DebitNote {
        debit_note_id: Option<String>,
        amount: BigDecimal,
        agreement_id: String,
        activity_id: String,
        allocation_id: String,
    },
}

pub struct BatchItem {
    pub payee_addr: String,
    pub payments: Vec<BatchPayment>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchPaymentAllocation {
    pub amount: BigDecimal,
    pub peer_obligation: HashMap<NodeId, Vec<BatchPaymentObligationAllocation>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchPayment {
    pub amount: BigDecimal,
    pub peer_obligation: HashMap<NodeId, Vec<BatchPaymentObligation>>,
}

#[derive(Queryable, Debug, Serialize, Identifiable, Insertable)]
#[table_name = "pay_batch_order"]
#[serde(rename_all = "camelCase")]
pub struct DbBatchOrder {
    pub id: String,
    pub ts: NaiveDateTime,
    pub owner_id: NodeId,
    pub payer_addr: String,
    pub platform: String,
    pub total_amount: BigDecimalField,
    pub paid_amount: BigDecimalField,
    pub paid: bool,
}

#[derive(Queryable, Debug, Serialize, Insertable)]
#[table_name = "pay_batch_order_item"]
#[serde(rename_all = "camelCase")]
pub struct DbBatchOrderItem {
    pub order_id: String,
    pub owner_id: String,
    pub payee_addr: String,
    pub allocation_id: String,
    pub amount: BigDecimalField,
    pub payment_id: Option<String>,
    pub paid: bool,
}

#[derive(Queryable, Debug, Serialize)]
pub struct DbAgreementBatchOrderItem {
    pub ts: NaiveDateTime,
    pub order_id: String,
    pub owner_id: String,
    pub payee_addr: String,
    pub allocation_id: String,
    pub amount: BigDecimalField,
    pub agreement_id: String,
    pub invoice_id: Option<String>,
    pub activity_id: Option<String>,
    pub debit_note_id: Option<String>,
}

#[derive(Queryable, Debug, Insertable)]
#[table_name = "pay_batch_order_item_document"]
pub struct DbBatchOrderItemAgreement {
    pub order_id: String,
    pub owner_id: NodeId,
    pub payee_addr: String,
    pub agreement_id: String,
    pub invoice_id: Option<String>,
    pub activity_id: String,
    pub debit_note_id: Option<String>,
}
