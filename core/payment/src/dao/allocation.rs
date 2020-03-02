use crate::error::DbResult;
use crate::models::*;
use crate::schema::pay_allocation::dsl;
use crate::schema::pay_payment::dsl as payment_dsl;
use bigdecimal::{BigDecimal, Zero};
use diesel::{self, ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};
use std::collections::HashMap;
use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};
use ya_persistence::types::{BigDecimalField, Summable};

pub struct AllocationDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for AllocationDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> AllocationDao<'c> {
    pub async fn create(&self, allocation: NewAllocation) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::pay_allocation)
                .values(allocation)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn get(&self, allocation_id: String) -> DbResult<Option<Allocation>> {
        do_with_transaction(self.pool, move |conn| {
            let allocation: Option<NewAllocation> = dsl::pay_allocation
                .find(allocation_id.clone())
                .first(conn)
                .optional()?;
            match allocation {
                Some(allocation) => {
                    let payments: Vec<BigDecimalField> = payment_dsl::pay_payment
                        .select(payment_dsl::amount)
                        .filter(payment_dsl::allocation_id.eq(allocation_id))
                        .load(conn)?;
                    let spent_amount = payments.sum();
                    let remaining_amount = &allocation.total_amount.0 - &spent_amount;
                    Ok(Some(Allocation {
                        allocation,
                        spent_amount,
                        remaining_amount,
                    }))
                }
                None => Ok(None),
            }
        })
        .await
    }

    pub async fn get_all(&self) -> DbResult<Vec<Allocation>> {
        do_with_transaction(self.pool, move |conn| {
            let allocations: Vec<NewAllocation> = dsl::pay_allocation.load(conn)?;
            let payments: Vec<BarePayment> = payment_dsl::pay_payment.load(conn)?;
            let mut payments_map = payments
                .into_iter()
                .fold(HashMap::new(), |mut map, payment| {
                    if let Some(allocation_id) = payment.allocation_id.clone() {
                        let x = map.entry(allocation_id).or_insert_with(BigDecimal::zero);
                        *x += Into::<BigDecimal>::into(payment.amount);
                    }
                    map
                });
            let allocations = allocations
                .into_iter()
                .map(|allocation| {
                    let spent_amount = payments_map
                        .remove(&allocation.id)
                        .unwrap_or(BigDecimal::zero());
                    let remaining_amount = &allocation.total_amount.0 - &spent_amount;
                    Allocation {
                        allocation,
                        spent_amount,
                        remaining_amount,
                    }
                })
                .collect();
            Ok(allocations)
        })
        .await
    }

    pub async fn delete(&self, allocation_id: String) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::delete(dsl::pay_allocation.filter(dsl::id.eq(allocation_id))).execute(conn)?;
            Ok(())
        })
        .await
    }
}
