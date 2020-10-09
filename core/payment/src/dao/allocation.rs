use crate::error::{DbError, DbResult};
use crate::models::allocation::{ReadObj, WriteObj};
use crate::schema::pay_allocation::dsl;
use bigdecimal::BigDecimal;
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
    pub async fn create(&self, allocation: NewAllocation, owner_id: NodeId) -> DbResult<String> {
        let allocation = WriteObj::new(allocation, owner_id);
        let allocation_id = allocation.id.clone();
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::pay_allocation)
                .values(allocation)
                .execute(conn)?;
            Ok(allocation_id)
        })
        .await
    }

    pub async fn get(
        &self,
        allocation_id: String,
        owner_id: NodeId,
    ) -> DbResult<Option<Allocation>> {
        readonly_transaction(self.pool, move |conn| {
            let allocation: Option<ReadObj> = dsl::pay_allocation
                .filter(dsl::owner_id.eq(owner_id))
                .filter(dsl::released.eq(false))
                .find(allocation_id)
                .first(conn)
                .optional()?;
            Ok(allocation.map(Into::into))
        })
        .await
    }

    pub async fn get_for_owner(&self, owner_id: NodeId) -> DbResult<Vec<Allocation>> {
        readonly_transaction(self.pool, move |conn| {
            let allocations: Vec<ReadObj> = dsl::pay_allocation
                .filter(dsl::owner_id.eq(owner_id))
                .filter(dsl::released.eq(false))
                .load(conn)?;
            Ok(allocations.into_iter().map(Into::into).collect())
        })
        .await
    }

    pub async fn release(&self, allocation_id: String, owner_id: NodeId) -> DbResult<bool> {
        do_with_transaction(self.pool, move |conn| {
            let num_released = diesel::update(
                dsl::pay_allocation
                    .filter(dsl::id.eq(allocation_id))
                    .filter(dsl::owner_id.eq(owner_id))
                    .filter(dsl::released.eq(false)),
            )
            .set(dsl::released.eq(true))
            .execute(conn)?;
            Ok(num_released > 0)
        })
        .await
    }

    pub async fn total_remaining_allocation(
        &self,
        platform: String,
        address: String,
    ) -> DbResult<BigDecimal> {
        readonly_transaction(self.pool, move |conn| {
            let total_remaining_amount = dsl::pay_allocation
                .select(dsl::remaining_amount)
                .filter(dsl::payment_platform.eq(platform))
                .filter(dsl::address.eq(address))
                .filter(dsl::released.eq(false))
                .get_results::<BigDecimalField>(conn)?
                .sum();

            Ok(total_remaining_amount)
        })
        .await
    }
}
