use bigdecimal::BigDecimal;
use chrono::Utc;
use diesel::{QueryDsl, RunQueryDsl};
use ya_client_model::{market::Agreement, payment::*, NodeId};
use ya_framework_mocks::market::FakeMarket;
use ya_persistence::{executor::DbExecutor, types::Role};

use crate::{dao::*, error::DbError, migrations};

pub const PLATFORM: &str = "erc20-hoodi-tglm";
pub const OTHER_PLATFORM: &str = "erc20-polygon-glm";
pub const ACTIVITY: &str = "payment-security-test-activity";

pub fn bind_acceptance_receiver() -> ya_framework_mocks::net::MockNet {
    use ya_core_model::payment::public::{AcceptDebitNote, AcceptInvoice, Ack};
    use ya_framework_mocks::net::{IMockNet, MockNet};
    let net = MockNet::new().bind();
    let prefix = "/security-tests/provider";
    net.register_node(&provider(), prefix);
    let address = format!("{prefix}/payment");
    ya_service_bus::typed::bind(&address, |_: AcceptDebitNote| async { Ok(Ack {}) });
    ya_service_bus::typed::bind(&address, |_: AcceptInvoice| async { Ok(Ack {}) });
    net
}

pub fn requestor() -> NodeId {
    "0x1000000000000000000000000000000000000001"
        .parse()
        .unwrap()
}

pub fn provider() -> NodeId {
    "0x2000000000000000000000000000000000000002"
        .parse()
        .unwrap()
}

pub fn database() -> DbExecutor {
    let db = DbExecutor::in_memory(&uuid::Uuid::new_v4().to_string()).unwrap();
    db.apply_migration(migrations::MIGRATIONS).unwrap();
    db
}

pub fn agreement() -> Agreement {
    FakeMarket::create_fake_agreement(requestor(), provider()).unwrap()
}

pub async fn seed_activity(db: &DbExecutor, agreement: &Agreement, role: Role) {
    let owner = match role {
        Role::Requestor => requestor(),
        Role::Provider => provider(),
    };
    db.as_dao::<AgreementDao>()
        .create_if_not_exists(agreement.clone(), owner, role.clone())
        .await
        .unwrap();
    db.as_dao::<ActivityDao>()
        .create_if_not_exists(ACTIVITY.into(), owner, role, agreement.agreement_id.clone())
        .await
        .unwrap();
}

pub fn debit_note(agreement: &Agreement) -> DebitNote {
    DebitNote {
        debit_note_id: uuid::Uuid::new_v4().to_string(),
        issuer_id: provider(),
        recipient_id: requestor(),
        payee_addr: provider().to_string(),
        payer_addr: requestor().to_string(),
        payment_platform: PLATFORM.into(),
        previous_debit_note_id: None,
        timestamp: Utc::now(),
        agreement_id: agreement.agreement_id.clone(),
        activity_id: ACTIVITY.into(),
        total_amount_due: 10.into(),
        usage_counter_vector: None,
        payment_due_date: None,
        status: DocumentStatus::Received,
    }
}

pub fn invoice(agreement: &Agreement) -> Invoice {
    Invoice {
        invoice_id: uuid::Uuid::new_v4().to_string(),
        issuer_id: provider(),
        recipient_id: requestor(),
        payee_addr: provider().to_string(),
        payer_addr: requestor().to_string(),
        payment_platform: PLATFORM.into(),
        timestamp: Utc::now(),
        agreement_id: agreement.agreement_id.clone(),
        activity_ids: vec![],
        amount: 10.into(),
        payment_due_date: Utc::now(),
        status: DocumentStatus::Received,
    }
}

pub async fn allocation(db: &DbExecutor, platform: &str) -> String {
    db.as_dao::<AllocationDao>()
        .create(
            NewAllocation {
                address: None,
                payment_platform: None,
                total_amount: 100.into(),
                timeout: None,
                make_deposit: false,
                deposit: None,
                extend_timeout: None,
            },
            requestor(),
            platform.into(),
            requestor().to_string(),
        )
        .await
        .unwrap()
}

pub async fn assert_acceptance_state(
    db: &DbExecutor,
    agreement_id: &str,
    allocation_id: &str,
    accepted: bool,
) {
    let amount = BigDecimal::from(if accepted { 10 } else { 0 });
    let agreement = db
        .as_dao::<AgreementDao>()
        .get(agreement_id.into(), requestor())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(agreement.total_amount_accepted.0, amount);
    assert_eq!(agreement.total_amount_scheduled.0, BigDecimal::from(0));
    let AllocationStatus::Active(allocation) = db
        .as_dao::<AllocationDao>()
        .get(allocation_id.into(), requestor())
        .await
        .unwrap()
    else {
        panic!("inactive allocation")
    };
    assert_eq!(allocation.spent_amount, amount);
    assert_eq!(allocation.remaining_amount, BigDecimal::from(100) - &amount);
    let count = db
        .with_transaction("count_expenditures", |conn| {
            Ok::<_, DbError>(
                crate::schema::pay_allocation_expenditure::table
                    .count()
                    .get_result::<i64>(conn)?,
            )
        })
        .await
        .unwrap();
    assert_eq!(count, i64::from(accepted));
}

pub static BUS_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
