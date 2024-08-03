use chrono::{NaiveDateTime, Timelike, Utc};
use serde::Serialize;
use crate::schema::pay_activity;
use ya_client_model::NodeId;
use ya_persistence::types::{BigDecimalField, Role};

#[derive(Debug, Insertable, Queryable, Identifiable, Serialize)]
#[table_name = "pay_activity"]
#[serde(rename_all = "camelCase")]
#[primary_key(id, owner_id)]
pub struct WriteObj {
    pub id: String,
    pub owner_id: NodeId,
    pub role: Role,
    pub agreement_id: String,
    pub total_amount_due: BigDecimalField,
    pub total_amount_accepted: BigDecimalField,
    pub total_amount_scheduled: BigDecimalField,
    pub total_amount_paid: BigDecimalField,
    pub created_ts: Option<NaiveDateTime>,
    pub updated_ts: Option<NaiveDateTime>,
}

impl WriteObj {
    pub fn new(id: String, owner_id: NodeId, role: Role, agreement_id: String) -> Self {
        let now = Utc::now();
        let created_ts = Some(now.naive_utc()).and_then(|v| v.with_nanosecond(0));
        let updated_ts = created_ts;
        Self {
            id,
            owner_id,
            role,
            agreement_id,
            total_amount_due: Default::default(),
            total_amount_accepted: Default::default(),
            total_amount_scheduled: Default::default(),
            total_amount_paid: Default::default(),
            created_ts,
            updated_ts,
        }
    }
}

#[derive(Queryable, Debug, Identifiable, Serialize)]
#[table_name = "pay_activity"]
#[serde(rename_all = "camelCase")]
#[primary_key(id, owner_id)]
pub struct ReadObj {
    pub id: String,
    pub owner_id: NodeId,
    pub role: Role,
    pub agreement_id: String,
    pub total_amount_due: BigDecimalField,
    pub total_amount_accepted: BigDecimalField,
    pub total_amount_scheduled: BigDecimalField,
    pub total_amount_paid: BigDecimalField,
    pub created_ts: Option<NaiveDateTime>,
    pub updated_ts: Option<NaiveDateTime>,

    pub peer_id: NodeId,    // From Agreement
    pub payee_addr: String, // From Agreement
    pub payer_addr: String, // From Agreement
}
