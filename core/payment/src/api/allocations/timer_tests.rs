use super::*;
use crate::test_support as fixture;
use actix_web::http::StatusCode;
use ya_core_model::driver::ValidateAllocationResult;
use ya_persistence::types::Role;

struct Fixture {
    db: DbExecutor,
    allocation: Allocation,
    agreement_id: String,
    tasks: AllocationReleaseTasks,
}

impl Fixture {
    async fn new(timeout: DateTime<Utc>) -> Self {
        let db = fixture::database();
        let agreement = fixture::agreement();
        fixture::seed_activity(&db, &agreement, Role::Requestor).await;
        let allocation_id = fixture::allocation(&db, fixture::PLATFORM).await;
        let mut allocation = active_allocation(&db, &allocation_id).await;
        allocation.timeout = Some(timeout);
        db.as_dao::<AllocationDao>()
            .replace(allocation, fixture::requestor())
            .await
            .unwrap();
        let allocation = active_allocation(&db, &allocation_id).await;
        let tasks = AllocationReleaseTasks::new_for_mocks_only();
        schedule_release_allocation(
            Data::new(db.clone()),
            allocation_id,
            fixture::requestor(),
            tasks.clone(),
        );
        Self {
            db,
            allocation,
            agreement_id: agreement.agreement_id,
            tasks,
        }
    }

    fn spend_during_validation(&self, amount: i64) {
        let db = self.db.clone();
        let allocation_id = self.allocation.allocation_id.clone();
        let agreement_id = self.agreement_id.clone();
        bus::bind(LOCAL_SERVICE, move |_: ValidateAllocation| {
            let db = db.clone();
            let allocation_id = allocation_id.clone();
            let agreement_id = agreement_id.clone();
            async move {
                db.as_dao::<AllocationDao>()
                    .spend_from_allocation_transaction(SpendFromAllocationArgs {
                        owner_id: fixture::requestor(),
                        allocation_id,
                        agreement_id,
                        activity_id: None,
                        amount: amount.into(),
                    })
                    .await
                    .unwrap();
                Ok::<_, ValidateAllocationError>(ValidateAllocationResult::Valid)
            }
        });
    }

    async fn amend(&self, total: i64, timeout: DateTime<Utc>) -> HttpResponse {
        amend_allocation(
            Data::new(self.db.clone()),
            Path::from(params::AllocationId {
                allocation_id: self.allocation.allocation_id.clone(),
            }),
            Json(AllocationUpdateRequest {
                total_amount: Some(JsonDecimal(total.into())),
                timeout: Some(timeout),
                deposit: None,
            }),
            Identity::admin(fixture::requestor()),
            Data::new(self.tasks.clone()),
        )
        .await
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        cancel_release_allocation_task(&self.allocation.allocation_id, self.tasks.clone());
    }
}

async fn active_allocation(db: &DbExecutor, id: &str) -> Allocation {
    match db
        .as_dao::<AllocationDao>()
        .get(id.into(), fixture::requestor())
        .await
        .unwrap()
    {
        AllocationStatus::Active(allocation) => allocation,
        _ => panic!("allocation is not active"),
    }
}

async fn await_release(mut task: tokio::task::JoinHandle<()>) {
    let result = tokio::time::timeout(Duration::from_secs(5), &mut task).await;
    task.abort();
    result.expect("allocation release timed out").unwrap();
}

#[actix_rt::test]
async fn rejected_amend_preserves_release_deadline_after_concurrent_spend() {
    let _guard = fixture::BUS_LOCK.lock().await;
    let f = Fixture::new(Utc::now() + chrono::Duration::hours(1)).await;
    f.spend_during_validation(60);
    let response = f.amend(50, Utc::now() + chrono::Duration::seconds(2)).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    tokio::time::sleep(Duration::from_secs(3)).await;
    let stored = active_allocation(&f.db, &f.allocation.allocation_id).await;
    assert_eq!(stored.total_amount, BigDecimal::from(100));
    assert_eq!(stored.spent_amount, BigDecimal::from(60));
    assert_eq!(stored.remaining_amount, BigDecimal::from(40));
    assert_eq!(stored.timeout, f.allocation.timeout);
}

#[actix_rt::test]
async fn successful_amend_reschedules_release_and_preserves_concurrent_spend() {
    let _guard = fixture::BUS_LOCK.lock().await;
    let f = Fixture::new(Utc::now() + chrono::Duration::hours(1)).await;
    f.spend_during_validation(30);
    let timeout = Utc::now() + chrono::Duration::seconds(2);
    let response = f.amend(50, timeout).await;
    assert_eq!(response.status(), StatusCode::OK);
    let stored = active_allocation(&f.db, &f.allocation.allocation_id).await;
    assert_eq!(stored.total_amount, BigDecimal::from(50));
    assert_eq!(stored.spent_amount, BigDecimal::from(30));
    assert_eq!(stored.remaining_amount, BigDecimal::from(20));
    assert_eq!(stored.timeout, Some(timeout));

    let task = f
        .tasks
        .tasks
        .lock()
        .remove(&f.allocation.allocation_id)
        .unwrap();
    await_release(task).await;
    assert!(!matches!(
        f.db.as_dao::<AllocationDao>()
            .get(f.allocation.allocation_id.clone(), fixture::requestor())
            .await
            .unwrap(),
        AllocationStatus::Active(_)
    ));
}

#[actix_rt::test]
async fn expiry_during_validation_does_not_recreate_release_task() {
    let _guard = fixture::BUS_LOCK.lock().await;
    let f = Fixture::new(Utc::now() + chrono::Duration::seconds(2)).await;
    let tasks = f.tasks.clone();
    let allocation_id = f.allocation.allocation_id.clone();
    bus::bind(LOCAL_SERVICE, move |_: ValidateAllocation| {
        let task = tasks.tasks.lock().remove(&allocation_id).unwrap();
        async move {
            await_release(task).await;
            Ok::<_, ValidateAllocationError>(ValidateAllocationResult::Valid)
        }
    });
    let response = f.amend(100, Utc::now() + chrono::Duration::hours(2)).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!matches!(
        f.db.as_dao::<AllocationDao>()
            .get(f.allocation.allocation_id.clone(), fixture::requestor())
            .await
            .unwrap(),
        AllocationStatus::Active(_)
    ));
    assert!(f.tasks.tasks.lock().is_empty());
}

#[actix_rt::test]
async fn expiry_and_amendment_respect_the_transaction_order() {
    let db = fixture::database();
    let id = fixture::allocation(&db, fixture::PLATFORM).await;
    let owner = fixture::requestor();
    let dao: AllocationDao = db.as_dao();
    let mut expired = active_allocation(&db, &id).await;
    expired.timeout = Some(Utc::now() - chrono::Duration::seconds(1));
    assert!(dao.replace(expired.clone(), owner).await.unwrap());

    // An old timer may reach the database after a timeout extension commits.
    let pending_expiry = dao.release_if_expired(id.clone(), owner);
    let mut extended = expired.clone();
    extended.timeout = Some(Utc::now() + chrono::Duration::hours(1));
    assert!(dao.replace(extended.clone(), owner).await.unwrap());
    assert_eq!(
        pending_expiry.await.unwrap(),
        extended.timeout.map(|t| t.naive_utc())
    );
    assert_eq!(active_allocation(&db, &id).await.timeout, extended.timeout);

    // If expiry commits first, a stale amendment cannot revive the allocation.
    assert!(dao.replace(expired, owner).await.unwrap());
    assert!(dao
        .release_if_expired(id.clone(), fixture::provider())
        .await
        .unwrap()
        .is_none());
    active_allocation(&db, &id).await;
    assert!(dao
        .release_if_expired(id.clone(), owner)
        .await
        .unwrap()
        .is_none());
    assert!(!dao.replace(extended, owner).await.unwrap());
    assert!(dao.release_if_expired(id, owner).await.unwrap().is_none());
}
