use crate::schema::pay_allocation;
use crate::DEFAULT_PAYMENT_PLATFORM;
use chrono::{NaiveDateTime, TimeZone, Utc};
use uuid::Uuid;
use ya_client_model::payment::{Allocation, NewAllocation};
use ya_client_model::NodeId;
use ya_persistence::types::BigDecimalField;

#[derive(Queryable, Debug, Identifiable, Insertable)]
#[table_name = "pay_allocation"]
pub struct WriteObj {
    pub id: String,
    pub owner_id: NodeId,
    pub payment_platform: String,
    pub address: String,
    pub total_amount: BigDecimalField,
    pub spent_amount: BigDecimalField,
    pub remaining_amount: BigDecimalField,
    pub timeout: Option<NaiveDateTime>,
    pub make_deposit: bool,
    pub released: bool,
}

impl WriteObj {
    pub fn new(allocation: NewAllocation, owner_id: NodeId) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            owner_id,
            payment_platform: allocation
                .payment_platform
                .unwrap_or(DEFAULT_PAYMENT_PLATFORM.to_string()),
            address: allocation.address.unwrap_or(owner_id.to_string()),
            total_amount: allocation.total_amount.clone().into(),
            spent_amount: Default::default(),
            remaining_amount: allocation.total_amount.into(),
            timeout: allocation.timeout.map(|v| v.naive_utc()),
            make_deposit: allocation.make_deposit,
            released: false,
        }
    }
}

impl From<WriteObj> for Allocation {
    fn from(allocation: WriteObj) -> Self {
        Self {
            allocation_id: allocation.id,
            address: allocation.address,
            payment_platform: allocation.payment_platform,
            total_amount: allocation.total_amount.into(),
            spent_amount: allocation.spent_amount.into(),
            remaining_amount: allocation.remaining_amount.into(),
            timeout: allocation.timeout.map(|v| Utc.from_utc_datetime(&v)),
            make_deposit: allocation.make_deposit,
        }
    }
}

pub type ReadObj = WriteObj;
