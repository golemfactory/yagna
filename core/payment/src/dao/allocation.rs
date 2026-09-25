use crate::error::{DbError, DbResult};
use crate::models::allocation::{AllocationExpenditureObj, AmendObj, ReadObj, WriteObj};
use crate::schema::pay_allocation::dsl;
use crate::schema::pay_allocation_expenditure::dsl as dsld;
use bigdecimal::BigDecimal;
use chrono::NaiveDateTime;
use diesel::{
    self, BoolExpressionMethods, ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl,
};
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

pub fn spend_from_allocation(conn: &mut ConnType, args: SpendFromAllocationArgs) -> DbResult<()> {
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

    let query = dsld::pay_allocation_expenditure
        .select(dsld::accepted_amount)
        .filter(dsld::owner_id.eq(args.owner_id))
        .filter(dsld::allocation_id.eq(&args.allocation_id))
        .filter(dsld::agreement_id.eq(&args.agreement_id))
        .into_boxed();

    let query = if let Some(activity_id) = &args.activity_id {
        query.filter(dsld::activity_id.eq(activity_id))
    } else {
        query.filter(dsld::activity_id.is_null())
    };

    if let Some(accepted_amount) = query.first::<BigDecimalField>(conn).optional()? {
        let new_document_amount: BigDecimalField = (accepted_amount.0 + &args.amount).into();

        let query = diesel::update(dsld::pay_allocation_expenditure)
            .set(dsld::accepted_amount.eq(new_document_amount))
            .filter(dsld::owner_id.eq(args.owner_id))
            .filter(dsld::allocation_id.eq(&args.allocation_id))
            .filter(dsld::agreement_id.eq(&args.agreement_id))
            .into_boxed();

        let query = if let Some(activity_id) = &args.activity_id {
            query.filter(dsld::activity_id.eq(activity_id))
        } else {
            query.filter(dsld::activity_id.is_null())
        };
        query.execute(conn)?;
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

impl AllocationDao<'_> {
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

    /// Applies an amend to an existing allocation.
    ///
    /// The caller reads the allocation, validates the change against the payment driver
    /// over GSB, and only then calls this. A spend can commit during that round trip, so
    /// `allocation.spent_amount` is stale by the time we get here and must not be written
    /// back — doing so would revert the spend while its `pay_allocation_expenditure` row
    /// survives, letting the same funds be committed twice.
    ///
    /// Instead we re-read `spent_amount` inside this transaction and derive `avail_amount`
    /// from the amended total and that fresh value, so a concurrent spend is preserved
    /// rather than rolled back. The `spent_amount` filter on the update is belt-and-braces
    /// on top of that: it pins the update to the row we just read, so if the storage engine
    /// ever let a write slip between the read and the update the result is a no-op rather
    /// than a lost spend.
    ///
    /// Returns `false` if the allocation is gone or released.
    pub async fn replace(&self, allocation: Allocation, owner_id: NodeId) -> DbResult<bool> {
        do_with_transaction(self.pool, "allocation_dao_replace", move |conn| {
            let current: Option<ReadObj> = dsl::pay_allocation
                .find((owner_id, allocation.allocation_id.clone()))
                .filter(dsl::released.eq(false))
                .first(conn)
                .optional()?;
            let current = match current {
                Some(current) => current,
                None => return Ok(false),
            };

            let avail_amount = allocation.total_amount.clone() - &current.spent_amount.0;
            if avail_amount < 0 {
                return Err(DbError::Query(format!(
                    "Amended allocation total {} is smaller than the already spent amount {}",
                    allocation.total_amount, current.spent_amount.0
                )));
            }

            let count = diesel::update(dsl::pay_allocation)
                .filter(dsl::id.eq(&allocation.allocation_id))
                .filter(dsl::owner_id.eq(&owner_id))
                .filter(dsl::released.eq(false))
                .filter(dsl::spent_amount.eq(&current.spent_amount))
                .set(AmendObj::new(&allocation, avail_amount.into()))
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

    pub async fn get_allocations_to_close(
        &self,
        owner_id: NodeId,
        platform: String,
    ) -> DbResult<Vec<Allocation>> {
        readonly_transaction(
            self.pool,
            "allocation_dao_get_allocations_to_close",
            move |conn| {
                let allocations: Vec<ReadObj> = dsl::pay_allocation
                    .filter(
                        dsl::owner_id
                            .eq(owner_id)
                            .and(dsl::released.eq(true))
                            .and(dsl::deposit.is_not_null())
                            .and(dsl::payment_platform.eq(platform))
                            .and(dsl::deposit_status.eq("open")),
                    )
                    .load(conn)?;
                Ok(allocations.into_iter().map(Into::into).collect())
            },
        )
        .await
    }

    pub async fn mark_allocation_closing(
        &self,
        allocation_id: String,
        owner_id: NodeId,
    ) -> DbResult<bool> {
        do_with_transaction(
            self.pool,
            "allocation_dao_mark_allocation_closing",
            move |conn| {
                let count = diesel::update(dsl::pay_allocation)
                    .filter(dsl::id.eq(allocation_id.clone()))
                    .filter(dsl::owner_id.eq(owner_id))
                    .filter(dsl::released.eq(true))
                    .filter(dsl::deposit.is_not_null())
                    .filter(dsl::deposit_status.eq("open"))
                    .set(dsl::deposit_status.eq("closing"))
                    .execute(conn)?;

                Ok(count == 1)
            },
        )
        .await
    }

    pub async fn mark_allocation_closed(
        &self,
        allocation_id: String,
        owner_id: NodeId,
    ) -> DbResult<bool> {
        do_with_transaction(
            self.pool,
            "allocation_dao_mark_allocation_closed",
            move |conn| {
                let count = diesel::update(dsl::pay_allocation)
                    .filter(dsl::id.eq(allocation_id.clone()))
                    .filter(dsl::owner_id.eq(owner_id))
                    .filter(dsl::released.eq(true))
                    .filter(dsl::deposit.is_not_null())
                    .filter(dsl::deposit_status.eq("closing"))
                    .set(dsl::deposit_status.eq("closed"))
                    .execute(conn)?;

                Ok(count == 1)
            },
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrations;
    use chrono::{Duration, Utc};
    use std::str::FromStr;
    use uuid::Uuid;
    use ya_client_model::payment::NewAllocation;
    use ya_persistence::executor::DbExecutor;

    fn owner() -> NodeId {
        NodeId::from_str("0xbabe000000000000000000000000000000000000").unwrap()
    }

    const AGREEMENT_ID: &str = "agreement-1";

    /// Creates an allocation plus the agreement its expenditures reference.
    async fn seed(db: &DbExecutor, total: u32) -> String {
        db.with_transaction("seed_agreement", move |conn| {
            let now = Utc::now().naive_utc();
            diesel::insert_into(crate::schema::pay_agreement::dsl::pay_agreement)
                .values((
                    crate::schema::pay_agreement::dsl::id.eq(AGREEMENT_ID),
                    crate::schema::pay_agreement::dsl::owner_id.eq(owner()),
                    crate::schema::pay_agreement::dsl::role.eq("R"),
                    crate::schema::pay_agreement::dsl::peer_id.eq(owner()),
                    crate::schema::pay_agreement::dsl::payee_addr.eq(owner().to_string()),
                    crate::schema::pay_agreement::dsl::payer_addr.eq(owner().to_string()),
                    crate::schema::pay_agreement::dsl::payment_platform.eq("erc20-holesky-tglm"),
                    crate::schema::pay_agreement::dsl::total_amount_due.eq("0"),
                    crate::schema::pay_agreement::dsl::total_amount_accepted.eq("0"),
                    crate::schema::pay_agreement::dsl::total_amount_scheduled.eq("0"),
                    crate::schema::pay_agreement::dsl::total_amount_paid.eq("0"),
                    crate::schema::pay_agreement::dsl::created_ts.eq(now),
                    crate::schema::pay_agreement::dsl::updated_ts.eq(now),
                ))
                .execute(conn)?;
            Ok::<_, DbError>(())
        })
        .await
        .unwrap();

        let dao: AllocationDao = db.as_dao();
        dao.create(
            NewAllocation {
                address: None,
                payment_platform: None,
                total_amount: BigDecimal::from(total),
                timeout: Some(Utc::now() + Duration::hours(1)),
                make_deposit: false,
                deposit: None,
                extend_timeout: None,
            },
            owner(),
            "erc20-holesky-tglm".to_string(),
            owner().to_string(),
        )
        .await
        .unwrap()
    }

    async fn balances(db: &DbExecutor, allocation_id: &str) -> (BigDecimal, BigDecimal) {
        let dao: AllocationDao = db.as_dao();
        match dao.get(allocation_id.to_string(), owner()).await.unwrap() {
            AllocationStatus::Active(a) => (a.remaining_amount, a.spent_amount),
            _ => panic!("allocation not active"),
        }
    }

    /// A spend that commits between the amend's read and its write must survive.
    ///
    /// The amend handler reads the allocation, validates the change against the payment
    /// driver over GSB, then writes back. Writing the pre-round-trip snapshot would
    /// revert the spend while its `pay_allocation_expenditure` row survives, letting the
    /// same funds be committed twice.
    #[actix_rt::test]
    async fn amend_does_not_revert_a_concurrent_spend() {
        let db = DbExecutor::in_memory(&format!("amend-race-{}", Uuid::new_v4())).unwrap();
        db.apply_migration(migrations::MIGRATIONS).unwrap();

        let allocation_id = seed(&db, 100).await;
        let dao: AllocationDao = db.as_dao();

        // The handler's read, before the GSB round trip.
        let stale = match dao.get(allocation_id.clone(), owner()).await.unwrap() {
            AllocationStatus::Active(a) => a,
            _ => panic!("allocation not active"),
        };
        assert_eq!(stale.spent_amount, BigDecimal::from(0));

        // A debit note lands while the driver is being consulted.
        dao.spend_from_allocation_transaction(SpendFromAllocationArgs {
            owner_id: owner(),
            allocation_id: allocation_id.clone(),
            agreement_id: AGREEMENT_ID.to_string(),
            activity_id: None,
            amount: BigDecimal::from(30),
        })
        .await
        .unwrap();

        // The amend now writes back, carrying the stale `spent_amount = 0`. The timeout
        // change must land, but the balance columns must reflect the spend, not the
        // snapshot.
        let amended = Allocation {
            timeout: Some(Utc::now() + Duration::hours(2)),
            ..stale
        };
        assert!(dao.replace(amended, owner()).await.unwrap());

        let (avail, spent) = balances(&db, &allocation_id).await;
        assert_eq!(spent, BigDecimal::from(30), "spend was reverted");
        assert_eq!(
            avail,
            BigDecimal::from(70),
            "avail was restored to the total"
        );
    }

    /// The ordinary path: no concurrent spend, so the amend applies and `avail_amount`
    /// is derived from the current `spent_amount` rather than reset to the total.
    #[actix_rt::test]
    async fn amend_applies_and_preserves_prior_spend() {
        let db = DbExecutor::in_memory(&format!("amend-ok-{}", Uuid::new_v4())).unwrap();
        db.apply_migration(migrations::MIGRATIONS).unwrap();

        let allocation_id = seed(&db, 100).await;
        let dao: AllocationDao = db.as_dao();

        dao.spend_from_allocation_transaction(SpendFromAllocationArgs {
            owner_id: owner(),
            allocation_id: allocation_id.clone(),
            agreement_id: AGREEMENT_ID.to_string(),
            activity_id: None,
            amount: BigDecimal::from(30),
        })
        .await
        .unwrap();

        let current = match dao.get(allocation_id.clone(), owner()).await.unwrap() {
            AllocationStatus::Active(a) => a,
            _ => panic!("allocation not active"),
        };

        // Raise the total to 150; nothing else races us.
        let amended = Allocation {
            total_amount: BigDecimal::from(150),
            timeout: Some(Utc::now() + Duration::hours(2)),
            ..current
        };
        assert!(dao.replace(amended, owner()).await.unwrap());

        let (avail, spent) = balances(&db, &allocation_id).await;
        assert_eq!(spent, BigDecimal::from(30), "spend must be untouched");
        assert_eq!(avail, BigDecimal::from(120), "avail must be total - spent");
    }

    /// Shrinking the total below what has already been spent must be rejected, using the
    /// spend visible inside the transaction rather than the caller's snapshot.
    #[actix_rt::test]
    async fn amend_below_spent_amount_is_rejected() {
        let db = DbExecutor::in_memory(&format!("amend-shrink-{}", Uuid::new_v4())).unwrap();
        db.apply_migration(migrations::MIGRATIONS).unwrap();

        let allocation_id = seed(&db, 100).await;
        let dao: AllocationDao = db.as_dao();

        let stale = match dao.get(allocation_id.clone(), owner()).await.unwrap() {
            AllocationStatus::Active(a) => a,
            _ => panic!("allocation not active"),
        };

        dao.spend_from_allocation_transaction(SpendFromAllocationArgs {
            owner_id: owner(),
            allocation_id: allocation_id.clone(),
            agreement_id: AGREEMENT_ID.to_string(),
            activity_id: None,
            amount: BigDecimal::from(60),
        })
        .await
        .unwrap();

        // Passed validation against the stale `spent_amount = 0`, but 50 < 60 spent.
        let amended = Allocation {
            total_amount: BigDecimal::from(50),
            ..stale
        };
        assert!(dao.replace(amended, owner()).await.is_err());

        let (avail, spent) = balances(&db, &allocation_id).await;
        assert_eq!(spent, BigDecimal::from(60));
        assert_eq!(avail, BigDecimal::from(40));
    }
}
