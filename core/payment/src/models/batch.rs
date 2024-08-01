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
        amount: BigDecimal,
        agreement_id: String,
        activity_id: String,
    },
}

pub struct BatchItem {
    pub payee_addr: String,
    pub payments: Vec<BatchPayment>,
}

pub struct BatchPayment {
    pub amount: BigDecimal,
    pub peer_obligation: HashMap<NodeId, Vec<BatchPaymentObligation>>,
}

#[derive(Queryable, Debug, Identifiable, Insertable)]
#[table_name = "pay_batch_order"]
pub struct DbBatchOrder {
    pub id: String,
    pub ts: NaiveDateTime,
    pub owner_id: NodeId,
    pub payer_addr: String,
    pub platform: String,
    pub total_amount: Option<f32>,
    pub paid: bool,
}

#[derive(Queryable, Debug, Identifiable, Insertable)]
#[table_name = "pay_batch_order_item"]
pub struct DbBatchOrderItem {
    pub id: String,
    pub payee_addr: String,
    pub amount: BigDecimalField,
    pub driver_order_id: Option<String>,
    pub paid: bool,
}

#[derive(Queryable, Debug, Identifiable, Insertable)]
#[table_name = "pay_batch_order_item_payment"]
pub struct DbBatchOrderItemPayment {
    pub id: String,
    pub payee_addr: String,
    pub payee_id: NodeId,
    pub json: String,
}
