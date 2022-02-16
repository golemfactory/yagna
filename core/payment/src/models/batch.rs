use std::collections::HashMap;
use std::str::FromStr;

use bigdecimal::BigDecimal;
use chrono::NaiveDateTime;
use diesel::backend::Backend;
use diesel::deserialize::FromSql;
use diesel::serialize::{Output, ToSql};
use diesel::sql_types::Text;
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

#[derive(Default)]
pub struct BatchPayment {
    pub amount: BigDecimal,
    pub peer_obligation: HashMap<NodeId, Vec<BatchPaymentObligation>>,
    pub payment_due_dates: HashMap<String, NaiveDateTime>,
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
    pub status: DbBatchOrderItemStatus,
    pub payment_due_date: Option<NaiveDateTime>,
}

#[derive(Queryable, Debug, Identifiable, Insertable)]
#[table_name = "pay_batch_order_item_payment"]
pub struct DbBatchOrderItemPayment {
    pub id: String,
    pub payee_addr: String,
    pub payee_id: NodeId,
    pub json: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, AsExpression, FromSqlRow)]
#[sql_type = "Text"]
pub enum DbBatchOrderItemStatus {
    Pending,
    Sent,
    Paid,
}

#[derive(Debug, thiserror::Error)]
#[error("Invalid batch order item status string: {0}")]
pub struct StatusParseError(pub String);

impl ToString for DbBatchOrderItemStatus {
    fn to_string(&self) -> String {
        match self {
            Self::Pending => "PENDING",
            Self::Sent => "SENT",
            Self::Paid => "PAID",
        }
        .to_string()
    }
}

impl FromStr for DbBatchOrderItemStatus {
    type Err = StatusParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "PENDING" => Self::Pending,
            "SENT" => Self::Sent,
            "PAID" => Self::Paid,
            _ => return Err(StatusParseError(s.to_string())),
        })
    }
}

impl<DB: Backend> ToSql<Text, DB> for DbBatchOrderItemStatus
where
    String: ToSql<Text, DB>,
{
    fn to_sql<W: std::io::Write>(&self, out: &mut Output<W, DB>) -> diesel::serialize::Result {
        self.to_string().to_sql(out)
    }
}

impl<DB> FromSql<Text, DB> for DbBatchOrderItemStatus
where
    String: FromSql<Text, DB>,
    DB: Backend,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> diesel::deserialize::Result<Self> {
        let s = String::from_sql(bytes)?;
        Ok(Self::from_str(&s)?)
    }
}
