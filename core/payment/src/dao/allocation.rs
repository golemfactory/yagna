use crate::error::{DbError, DbResult};
use crate::models::allocation::{ReadObj, WriteObj};
use crate::schema::pay_allocation::dsl;
use bigdecimal::BigDecimal;
use chrono::NaiveDateTime;
use diesel::{self, ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};
use ya_client_model::payment::{Allocation, NewAllocation};
use ya_client_model::NodeId;
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};
use ya_persistence::types::{BigDecimalField, Summable};

pub struct AllocationDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for AllocationDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

pub fn spend_from_allocation(
    allocation_id: &String,
    amount: &BigDecimalField,
    conn: &ConnType,
) -> DbResult<()> {
    let allocation: ReadObj = dsl::pay_allocation.find(allocation_id).first(conn)?;
    if amount > &allocation.remaining_amount {
        return Err(DbError::Query(format!(
            "Not enough funds in allocation. Needed: {} Remaining: {}",
            amount, allocation.remaining_amount
        )));
    }
    let spent_amount = &allocation.spent_amount + amount;
    let remaining_amount = &allocation.remaining_amount - amount;
    diesel::update(&allocation)
        .set((
            dsl::spent_amount.eq(spent_amount),
            dsl::remaining_amount.eq(remaining_amount),
        ))
        .execute(conn)?;
    Ok(())
}

impl<'c> AllocationDao<'c> {
    pub async fn create(
        &self,
        allocation: NewAllocation,
        owner_id: NodeId,
        payment_platform: String,
        address: String,
    ) -> DbResult<String> {
        let allocation = WriteObj::new(allocation, owner_id, payment_platform, address);
        let allocation_id = allocation.id.clone();
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::pay_allocation)
                .values(allocation)
                .execute(conn)?;
            Ok(allocation_id)
        })
        .await
    }

    pub async fn get(&self, allocation_id: String, owner_id: NodeId) -> DbResult<AllocationStatus> {
        readonly_transaction(self.pool, move |conn| {
            let allocation: Option<ReadObj> = dsl::pay_allocation
                .filter(dsl::owner_id.eq(owner_id))
                .filter(dsl::released.eq(false))
                .find(allocation_id)
                .first(conn)
                .optional()?;

            if let Some(allocation) = allocation {
                return if !allocation.released {
                    Ok(AllocationStatus::Active(allocation.into()))
                } else {
                    Ok(AllocationStatus::Gone)
                };
            }
            Ok(AllocationStatus::NotFound)
        })
        .await
    }

    pub async fn get_many(
        &self,
        allocation_ids: Vec<String>,
        owner_id: NodeId,
    ) -> DbResult<Vec<Allocation>> {
        readonly_transaction(self.pool, move |conn| {
            let allocations: Vec<ReadObj> = dsl::pay_allocation
                .filter(dsl::owner_id.eq(owner_id))
                .filter(dsl::released.eq(false))
                .filter(dsl::id.eq_any(allocation_ids))
                .load(conn)?;
            Ok(allocations.into_iter().map(Into::into).collect())
        })
        .await
    }

    pub async fn get_for_owner(
        &self,
        owner_id: NodeId,
        after_timestamp: Option<NaiveDateTime>,
        max_items: Option<u32>,
    ) -> DbResult<Vec<Allocation>> {
        self.get_filtered(Some(owner_id), after_timestamp, max_items, None, None)
            .await
    }

    pub async fn get_for_address(
        &self,
        payment_platform: String,
        address: String,
    ) -> DbResult<Vec<Allocation>> {
        self.get_filtered(None, None, None, Some(payment_platform), Some(address))
            .await
    }

    pub async fn get_filtered(
        &self,
        owner_id: Option<NodeId>,
        after_timestamp: Option<NaiveDateTime>,
        max_items: Option<u32>,
        payment_platform: Option<String>,
        address: Option<String>,
    ) -> DbResult<Vec<Allocation>> {
        readonly_transaction(self.pool, move |conn| {
            let mut query = dsl::pay_allocation
                .filter(dsl::released.eq(false))
                .into_boxed();
            if let Some(owner_id) = owner_id {
                query = query.filter(dsl::owner_id.eq(owner_id))
            }
            if let Some(after_timestamp) = after_timestamp {
                query = query.filter(dsl::timestamp.gt(after_timestamp))
            }
            if let Some(payment_platform) = payment_platform {
                query = query.filter(dsl::timestamp.gt(payment_platform))
            }
            if let Some(address) = address {
                query = query.filter(dsl::timestamp.gt(address))
            }
            if let Some(max_items) = max_items {
                query = query.limit(max_items.into())
            }
            let allocations: Vec<ReadObj> = query.load(conn)?;
            Ok(allocations.into_iter().map(Into::into).collect())
        })
        .await
    }

    pub async fn release(
        &self,
        allocation_id: String,
        owner_id: Option<NodeId>,
    ) -> DbResult<AllocationReleaseStatus> {
        let id = allocation_id.clone();
        do_with_transaction(self.pool, move |conn| {
            let allocation: Option<ReadObj> = dsl::pay_allocation
                .find(id.clone())
                .first(conn)
                .optional()?;

            match allocation {
                Some(allocation) => {
                    if let Some(owner_id) = owner_id {
                        if owner_id != allocation.owner_id {
                            return Ok(AllocationReleaseStatus::NotFound);
                        }
                    }

                    if allocation.released {
                        return Ok(AllocationReleaseStatus::Gone);
                    }
                }
                None => return Ok(AllocationReleaseStatus::NotFound),
            }

            let num_released = diesel::update(dsl::pay_allocation)
                .filter(dsl::released.eq(false))
                .filter(dsl::id.eq(id.clone()))
                .set(dsl::released.eq(true))
                .execute(conn)?;

            return match num_released {
                1 => Ok(AllocationReleaseStatus::Released),
                _ => Err(DbError::Query(format!(
                    "Update error occurred when releasing allocation {}",
                    allocation_id
                ))),
            };
        })
        .await
    }

    pub async fn total_remaining_allocation(
        &self,
        platform: String,
        address: String,
        after_timestamp: NaiveDateTime,
    ) -> DbResult<BigDecimal> {
        readonly_transaction(self.pool, move |conn| {
            let total_remaining_amount = dsl::pay_allocation
                .select(dsl::remaining_amount)
                .filter(dsl::payment_platform.eq(platform))
                .filter(dsl::address.eq(address))
                .filter(dsl::released.eq(false))
                .filter(dsl::timestamp.gt(after_timestamp))
                .get_results::<BigDecimalField>(conn)?
                .sum();

            Ok(total_remaining_amount)
        })
        .await
    }
}

pub enum AllocationStatus {
    Active(Allocation),
    Gone,
    NotFound,
}

pub enum AllocationReleaseStatus {
    Gone,
    NotFound,
    Released,
}
