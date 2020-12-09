// Extrnal crates
use actix_web::web::{get, Data, Path, Query};
use actix_web::{HttpResponse, Scope};

// Workspace uses
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

// Local uses
use crate::api::*;
use crate::dao::*;
use crate::utils::*;

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .route("/payments", get().to(get_payments))
        .route("/payments/{payment_id}", get().to(get_payment))
}

async fn get_payments(
    db: Data<DbExecutor>,
    query: Query<EventParams>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let timeout_secs = query.timeout;
    let later_than = query.later_than.map(|d| d.naive_utc());

    let dao: PaymentDao = db.as_dao();
    let getter = || async {
        dao.get_for_provider(node_id.clone(), later_than.clone())
            .await
    };

    match listen_for_events(getter, timeout_secs).await {
        Ok(payments) => response::ok(payments),
        Err(e) => response::server_error(&e),
    }
}

async fn get_payment(db: Data<DbExecutor>, path: Path<PaymentId>, id: Identity) -> HttpResponse {
    let payment_id = path.payment_id.clone();
    let node_id = id.identity;
    let dao: PaymentDao = db.as_dao();
    match dao.get(payment_id, node_id).await {
        Ok(Some(payment)) => response::ok(payment),
        Ok(None) => response::not_found(),
        Err(e) => response::server_error(&e),
    }
}
