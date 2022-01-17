// Extrnal crates
use actix_web::web::{get, Data, Path, Query};
use actix_web::{HttpResponse, Scope};
use std::str::FromStr;

// Workspace uses
use ya_client_model::payment::*;
use ya_core_model::payment::local::{NetworkName, DriverName};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

// Local uses
use crate::dao::*;
use crate::utils::*;

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .route("/payments", get().to(get_payments))
        .route("/payments/{payment_id}", get().to(get_payment))
}

async fn get_payments(
    db: Data<DbExecutor>,
    query: Query<params::DriverNetworkParams>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let timeout_secs = query.event_params.timeout.unwrap_or(params::DEFAULT_EVENT_TIMEOUT);
    let after_timestamp = query.event_params.after_timestamp.map(|d| d.naive_utc());
    let network = query.network.as_ref().map(|n| NetworkName::from_str(n.as_str()).unwrap());
    let driver = query.driver.as_ref().map(|d| DriverName::from_str(d.as_str()).unwrap());
    let max_events = query.event_params.max_events;
    let app_session_id = &query.event_params.app_session_id;

    let dao: PaymentDao = db.as_dao();
    let getter = || async {
        dao.get_for_node_id(
            node_id.clone(),
            after_timestamp.clone(),
            max_events.clone(),
            app_session_id.clone(),
            network.clone(),
            driver.clone(),
        )
        .await
    };

    match listen_for_events(getter, timeout_secs).await {
        Ok(payments) => response::ok(payments),
        Err(e) => response::server_error(&e),
    }
}

async fn get_payment(
    db: Data<DbExecutor>,
    path: Path<params::PaymentId>,
    id: Identity,
) -> HttpResponse {
    let payment_id = path.payment_id.clone();
    let node_id = id.identity;
    let dao: PaymentDao = db.as_dao();
    match dao.get(payment_id, node_id).await {
        Ok(Some(payment)) => response::ok(payment),
        Ok(None) => response::not_found(),
        Err(e) => response::server_error(&e),
    }
}
