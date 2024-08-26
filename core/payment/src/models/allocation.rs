use crate::schema::pay_allocation;
use chrono::{Days, NaiveDateTime, TimeZone, Utc};
use uuid::Uuid;
use ya_client_model::payment::{Allocation, NewAllocation};
use ya_client_model::NodeId;
use ya_persistence::types::BigDecimalField;

#[derive(Queryable, Debug, Identifiable, Insertable, AsChangeset)]
#[table_name = "pay_allocation"]
pub struct WriteObj {
    pub id: String,
    pub owner_id: NodeId,
    pub payment_platform: String,
    pub address: String,
    pub avail_amount: BigDecimalField,
    pub spent_amount: BigDecimalField,
    pub created_ts: NaiveDateTime,
    pub updated_ts: NaiveDateTime,
    pub timeout: NaiveDateTime,
    pub deposit: Option<String>,
    pub released: bool,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "pay_allocation"]
pub struct ReadObj {
    pub id: String,
    pub owner_id: NodeId,
    pub payment_platform: String,
    pub address: String,
    pub avail_amount: BigDecimalField,
    pub spent_amount: BigDecimalField,
    pub created_ts: NaiveDateTime,
    pub updated_ts: NaiveDateTime,
    pub timeout: NaiveDateTime,
    pub deposit: Option<String>,
    pub released: bool,
}

impl WriteObj {
    pub fn new(
        allocation: NewAllocation,
        owner_id: NodeId,
        payment_platform: String,
        address: String,
    ) -> Self {
        let now = Utc::now().naive_utc();
        Self {
            id: Uuid::new_v4().to_string(),
            owner_id,
            payment_platform,
            address,
            avail_amount: allocation.total_amount.clone().into(),
            spent_amount: Default::default(),
            created_ts: now,
            updated_ts: now,
            timeout: allocation.timeout.map(|v| v.naive_utc()).unwrap_or(Utc::now().checked_add_days(Days::new(365 * 10)).unwrap().naive_utc()),
            deposit: allocation
                .deposit
                .map(|deposit| serde_json::to_string(&deposit).unwrap()),
            released: false,
        }
    }

    pub fn from_allocation(allocation: Allocation, owner_id: NodeId) -> Self {
        Self {
            id: allocation.allocation_id,
            owner_id,
            payment_platform: allocation.payment_platform,
            address: allocation.address,
            avail_amount: (allocation.total_amount.clone() - allocation.spent_amount.clone()).into(),
            spent_amount: allocation.spent_amount.into(),
            created_ts: allocation.timestamp.naive_utc(),
            updated_ts: allocation.timestamp.naive_utc(),
            timeout: allocation.timeout.map(|v| v.naive_utc()).unwrap_or(Utc::now().checked_add_days(Days::new(365 * 10)).unwrap().naive_utc()),
            deposit: allocation
                .deposit
                .map(|deposit| serde_json::to_string(&deposit).unwrap()),
            released: false,
        }
    }
}

impl From<ReadObj> for Allocation {
    fn from(allocation: ReadObj) -> Self {
        Self {
            allocation_id: allocation.id,
            address: allocation.address,
            payment_platform: allocation.payment_platform,
            total_amount: (allocation.avail_amount.clone() + allocation.spent_amount.clone()).into(),
            spent_amount: allocation.spent_amount.into(),
            remaining_amount: allocation.avail_amount.into(),
            timestamp: Utc.from_utc_datetime(&allocation.updated_ts),
            timeout: Some(Utc.from_utc_datetime(&allocation.timeout)),
            make_deposit: false,
            deposit: allocation
                .deposit
                .and_then(|s| serde_json::from_str(&s).ok()),
            extend_timeout: None,
        }
    }
}
