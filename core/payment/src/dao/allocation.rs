use crate::error::{DbError, DbResult};
use crate::models::allocation::{AllocationExpenditureObj, ReadObj, WriteObj};
use crate::schema::pay_allocation::dsl;
use crate::schema::pay_allocation_expenditure::dsl as dsld;
use bigdecimal::BigDecimal;
use chrono::NaiveDateTime;
use diesel::{self, ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};
use ya_client_model::payment::allocation::{AllocationExpenditure, Deposit};
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

pub struct SpendFromAllocationArgs {
    pub owner_id: NodeId,
    pub allocation_id: String,
    pub agreement_id: String,
    pub activity_id: Option<String>,
    pub amount: BigDecimal,
}

pub fn spend_from_allocation(conn: &ConnType, args: SpendFromAllocationArgs) -> DbResult<()> {
    let allocation: ReadObj = dsl::pay_allocation
        .find((args.owner_id, args.allocation_id.clone()))
        .first(conn)?;
    if args.amount > allocation.avail_amount.0 {
        return Err(DbError::Query(format!(
            "Not enough funds in allocation. Needed: {} Remaining: {}",
            args.amount, allocation.avail_amount.0
        )));
    }
    let spent_amount: BigDecimalField = (allocation.spent_amount.0 + &args.amount).into();
    let avail_amount: BigDecimalField = (allocation.avail_amount.0 - &args.amount).into();
    diesel::update(dsl::pay_allocation)
        .set((
            dsl::spent_amount.eq(spent_amount),
            dsl::avail_amount.eq(avail_amount),
        ))
        .filter(dsl::id.eq(&args.allocation_id))
        .filter(dsl::owner_id.eq(args.owner_id))
        .execute(conn)?;

    if let Some(accepted_amount) = dsld::pay_allocation_expenditure
        .select(dsld::accepted_amount)
        .filter(dsld::owner_id.eq(args.owner_id))
        .filter(dsld::allocation_id.eq(&args.allocation_id))
        .filter(dsld::agreement_id.eq(&args.agreement_id))
        .filter(dsld::activity_id.eq(&args.activity_id))
        .first::<BigDecimalField>(conn)
        .optional()?
    {
        let new_document_amount: BigDecimalField = (accepted_amount.0 + &args.amount).into();
        diesel::update(dsld::pay_allocation_expenditure)
            .set(dsld::accepted_amount.eq(new_document_amount))
            .filter(dsld::owner_id.eq(args.owner_id))
            .filter(dsld::allocation_id.eq(&args.allocation_id))
            .filter(dsld::agreement_id.eq(&args.agreement_id))
            .filter(dsld::activity_id.eq(&args.activity_id))
            .execute(conn)?;
    } else {
        diesel::insert_into(dsld::pay_allocation_expenditure)
            .values((
                dsld::owner_id.eq(args.owner_id),
                dsld::allocation_id.eq(&args.allocation_id),
                dsld::agreement_id.eq(&args.agreement_id),
                dsld::activity_id.eq(&args.activity_id),
                dsld::accepted_amount.eq(BigDecimalField::from(args.amount)),
                dsld::scheduled_amount.eq(BigDecimalField::default()),
            ))
            .execute(conn)?;
    }

    Ok(())
}

impl<'c> AllocationDao<'c> {
    pub async fn spend_from_allocation_transaction(
        &self,
        args: SpendFromAllocationArgs,
    ) -> DbResult<()> {
        do_with_transaction(self.pool, "spend_from_allocation_transaction", |conn| {
            spend_from_allocation(conn, args)
        })
        .await
    }

    pub async fn get_expenditures(
        &self,
        owner_id: NodeId,
        allocation_id: String,
    ) -> DbResult<Vec<AllocationExpenditure>> {
        readonly_transaction(self.pool, "allocation_dao_get_expenditures", move |conn| {
            let r: Vec<AllocationExpenditureObj> = dsld::pay_allocation_expenditure
                .filter(dsld::owner_id.eq(owner_id))
                .filter(dsld::allocation_id.eq(allocation_id))
                .load(conn)?;
            Ok(r.into_iter().map(Into::into).collect())
        })
        .await
    }

    pub async fn create(
        &self,
        allocation: NewAllocation,
        owner_id: NodeId,
        payment_platform: String,
        address: String,
    ) -> DbResult<String> {
        let allocation = WriteObj::new(allocation, owner_id, payment_platform, address);
        let allocation_id = allocation.id.clone();
        do_with_transaction(self.pool, "allocation_dao_create", move |conn| {
            diesel::insert_into(dsl::pay_allocation)
                .values(allocation)
                .execute(conn)?;
            Ok(allocation_id)
        })
        .await
    }

    pub async fn replace(&self, allocation: Allocation, owner_id: NodeId) -> DbResult<bool> {
        do_with_transaction(self.pool, "allocation_dao_replace", move |conn| {
            let count = diesel::update(dsl::pay_allocation)
                .filter(dsl::id.eq(allocation.allocation_id.clone()))
                .filter(dsl::owner_id.eq(&owner_id))
                .filter(dsl::released.eq(false))
                .set(WriteObj::from_allocation(allocation, owner_id))
                .execute(conn)?;

            Ok(count == 1)
        })
        .await
    }

    pub async fn get(&self, allocation_id: String, owner_id: NodeId) -> DbResult<AllocationStatus> {
        readonly_transaction(self.pool, "allocation_dao_get", move |conn| {
            let allocation: Option<ReadObj> = dsl::pay_allocation
                .filter(dsl::owner_id.eq(owner_id))
                .filter(dsl::released.eq(false))
                .find((owner_id, allocation_id))
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
        readonly_transaction(self.pool, "allocation_dao_get_many", move |conn| {
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
        released: Option<bool>,
    ) -> DbResult<Vec<Allocation>> {
        self.get_filtered(
            Some(owner_id),
            after_timestamp,
            max_items,
            None,
            None,
            released,
        )
        .await
    }

    pub async fn get_for_address(
        &self,
        payment_platform: String,
        address: String,
        released: Option<bool>,
    ) -> DbResult<Vec<Allocation>> {
        self.get_filtered(
            None,
            None,
            None,
            Some(payment_platform),
            Some(address),
            released,
        )
        .await
    }

    pub async fn get_filtered(
        &self,
        owner_id: Option<NodeId>,
        after_timestamp: Option<NaiveDateTime>,
        max_items: Option<u32>,
        payment_platform: Option<String>,
        address: Option<String>,
        released: Option<bool>,
    ) -> DbResult<Vec<Allocation>> {
        readonly_transaction(self.pool, "allocation_dao_get_filtered", move |conn| {
            let mut query = dsl::pay_allocation.into_boxed();
            if let Some(released) = released {
                query = query.filter(dsl::released.eq(released));
            }
            if let Some(owner_id) = owner_id {
                query = query.filter(dsl::owner_id.eq(owner_id))
            }
            if let Some(after_timestamp) = after_timestamp {
                query = query.filter(dsl::timeout.gt(after_timestamp))
            }
            if let Some(payment_platform) = payment_platform {
                query = query.filter(dsl::payment_platform.eq(payment_platform))
            }
            if let Some(address) = address {
                query = query.filter(dsl::address.eq(address))
            }
            if let Some(max_items) = max_items {
                query = query.limit(max_items.into())
            }
            let allocations: Vec<ReadObj> = query.order_by(dsl::updated_ts.asc()).load(conn)?;
            Ok(allocations.into_iter().map(Into::into).collect())
        })
        .await
    }

    pub async fn release(
        &self,
        allocation_id: String,
        owner_id: NodeId,
    ) -> DbResult<AllocationReleaseStatus> {
        let id = allocation_id.clone();
        do_with_transaction(self.pool, "allocation_dao_release", move |conn| {
            let allocation: Option<ReadObj> = dsl::pay_allocation
                .find((owner_id, allocation_id.clone()))
                .first(conn)
                .optional()?;

            let (deposit, platform) = match allocation {
                Some(allocation) => {
                    if owner_id != allocation.owner_id {
                        return Ok(AllocationReleaseStatus::NotFound);
                    }

                    if allocation.released {
                        return Ok(AllocationReleaseStatus::Gone);
                    }

                    let allocation = Allocation::from(allocation);

                    (allocation.deposit, allocation.payment_platform)
                }
                None => return Ok(AllocationReleaseStatus::NotFound),
            };

            let num_released = diesel::update(dsl::pay_allocation)
                .filter(dsl::released.eq(false))
                .filter(dsl::id.eq(id.clone()))
                .set(dsl::released.eq(true))
                .execute(conn)?;

            match num_released {
                1 => Ok(AllocationReleaseStatus::Released { deposit, platform }),
                _ => Err(DbError::Query(format!(
                    "Update error occurred when releasing allocation {}",
                    allocation_id
                ))),
            }
        })
        .await
    }

    pub async fn total_remaining_allocation(
        &self,
        platform: String,
        address: String,
        after_timestamp: NaiveDateTime,
    ) -> DbResult<BigDecimal> {
        readonly_transaction(
            self.pool,
            "allocation_dao_total_remaining_allocation",
            move |conn| {
                let total_remaining_amount = dsl::pay_allocation
                    .select(dsl::avail_amount)
                    .filter(dsl::payment_platform.eq(platform))
                    .filter(dsl::address.eq(address))
                    .filter(dsl::released.eq(false))
                    .filter(dsl::timeout.gt(after_timestamp))
                    .get_results::<BigDecimalField>(conn)?
                    .sum();

                Ok(total_remaining_amount)
            },
        )
        .await
    }
}

#[allow(clippy::large_enum_variant)]
pub enum AllocationStatus {
    Active(Allocation),
    Gone,
    NotFound,
}

pub enum AllocationReleaseStatus {
    Gone,
    NotFound,
    Released {
        deposit: Option<Deposit>,
        platform: String,
    },
}
