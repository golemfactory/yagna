use crate::schema::{pay_allocation, pay_allocation_expenditure};
use chrono::{Days, NaiveDateTime, TimeZone, Utc};
use uuid::Uuid;
use ya_client_model::payment::allocation::AllocationExpenditure;
use ya_client_model::payment::{Allocation, NewAllocation};
use ya_client_model::NodeId;
use ya_persistence::types::BigDecimalField;

// Deliberately not `AsChangeset`: as a full-row changeset this would overwrite
// `spent_amount`/`avail_amount` from whatever snapshot the caller happens to hold. Use
// `AmendObj` for updates.
#[derive(Queryable, Debug, Identifiable, Insertable)]
#[diesel(table_name = pay_allocation)]
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
    pub deposit_status: Option<String>,
    pub released: bool,
}

#[derive(Queryable, Debug, Identifiable)]
#[diesel(table_name = pay_allocation)]
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
    pub released: bool,
    pub deposit: Option<String>,
    pub deposit_status: Option<String>,
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
            timeout: allocation.timeout.map(|v| v.naive_utc()).unwrap_or(
                Utc::now()
                    .checked_add_days(Days::new(365 * 10))
                    .unwrap()
                    .naive_utc(),
            ),
            deposit: allocation
                .deposit
                .as_ref()
                .map(|deposit| serde_json::to_string(&deposit).unwrap()),
            deposit_status: allocation.deposit.map(|_| "open".to_string()),
            released: false,
        }
    }
}

/// Changeset for amending an existing allocation.
///
/// Deliberately narrow: it carries only the columns the amend endpoint owns. In
/// particular `spent_amount` is never written, because the amend handler makes a GSB
/// round trip to the payment driver between reading the allocation and writing it back,
/// and a spend committed in that window must not be rolled back. `avail_amount` is
/// recomputed by the DAO from the *current* `spent_amount`, read inside the same
/// transaction as the update.
///
/// `deposit` is `None` when the allocation has no deposit; diesel's default
/// `AsChangeset` behaviour skips `None` columns, which is what we want here — amend can
/// never add or remove a deposit, only update the `validate` flag of an existing one.
#[derive(AsChangeset)]
#[diesel(table_name = pay_allocation)]
pub struct AmendObj {
    pub avail_amount: BigDecimalField,
    pub timeout: NaiveDateTime,
    pub deposit: Option<String>,
}

impl AmendObj {
    pub fn new(allocation: &Allocation, avail_amount: BigDecimalField) -> Self {
        Self {
            avail_amount,
            timeout: allocation.timeout.map(|v| v.naive_utc()).unwrap_or(
                Utc::now()
                    .checked_add_days(Days::new(365 * 10))
                    .unwrap()
                    .naive_utc(),
            ),
            deposit: allocation
                .deposit
                .as_ref()
                .map(|deposit| serde_json::to_string(&deposit).unwrap()),
        }
    }
}

impl From<ReadObj> for Allocation {
    fn from(allocation: ReadObj) -> Self {
        Self {
            allocation_id: allocation.id,
            address: allocation.address,
            payment_platform: allocation.payment_platform,
            total_amount: (allocation.avail_amount.clone() + allocation.spent_amount.clone())
                .into(),
            spent_amount: allocation.spent_amount.into(),
            remaining_amount: allocation.avail_amount.into(),
            timestamp: Utc.from_utc_datetime(&allocation.updated_ts),
            timeout: Some(Utc.from_utc_datetime(&allocation.timeout)),
            make_deposit: false,
            deposit: allocation
                .deposit
                .and_then(|s| serde_json::from_str(&s).ok()),
            extend_timeout: None,
            created_ts: Utc.from_utc_datetime(&allocation.created_ts),
            updated_ts: Utc.from_utc_datetime(&allocation.updated_ts),
        }
    }
}

#[derive(Queryable, Debug, Clone, Insertable, AsChangeset)]
#[diesel(table_name = pay_allocation_expenditure)]
pub struct AllocationExpenditureObj {
    pub owner_id: NodeId,
    pub allocation_id: String,
    pub agreement_id: String,
    pub activity_id: Option<String>,
    pub accepted_amount: BigDecimalField,
    pub scheduled_amount: BigDecimalField,
}

impl From<AllocationExpenditureObj> for AllocationExpenditure {
    fn from(expenditure: AllocationExpenditureObj) -> Self {
        Self {
            allocation_id: expenditure.allocation_id,
            agreement_id: expenditure.agreement_id,
            activity_id: expenditure.activity_id,
            accepted_amount: expenditure.accepted_amount.0,
            scheduled_amount: expenditure.scheduled_amount.0,
        }
    }
}
