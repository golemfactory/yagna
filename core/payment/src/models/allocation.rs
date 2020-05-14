use crate::schema::pay_allocation;
use chrono::{NaiveDateTime, TimeZone, Utc};
use uuid::Uuid;
use ya_client_model::payment::{Allocation, NewAllocation};
use ya_core_model::ethaddr::NodeId;
use ya_persistence::types::BigDecimalField;

#[derive(Queryable, Debug, Identifiable, Insertable)]
#[table_name = "pay_allocation"]
pub struct WriteObj {
    pub id: String,
    pub owner_id: NodeId,
    pub total_amount: BigDecimalField,
    pub spent_amount: BigDecimalField,
    pub remaining_amount: BigDecimalField,
    pub timeout: Option<NaiveDateTime>,
    pub make_deposit: bool,
}

impl WriteObj {
    pub fn new(allocation: NewAllocation, owner_id: NodeId) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            owner_id,
            total_amount: allocation.total_amount.clone().into(),
            spent_amount: Default::default(),
            remaining_amount: allocation.total_amount.into(),
            timeout: allocation.timeout.map(|v| v.naive_utc()),
            make_deposit: allocation.make_deposit,
        }
    }
}

impl From<WriteObj> for Allocation {
    fn from(allocation: WriteObj) -> Self {
        Self {
            allocation_id: allocation.id,
            total_amount: allocation.total_amount.into(),
            spent_amount: allocation.spent_amount.into(),
            remaining_amount: allocation.remaining_amount.into(),
            timeout: allocation.timeout.map(|v| Utc.from_utc_datetime(&v)),
            make_deposit: allocation.make_deposit,
        }
    }
}

pub type ReadObj = WriteObj;
