// External crates
use actix_web::web::{get, Data, Path, Query};
use actix_web::{HttpResponse, Scope};
use std::str::FromStr;
use ya_service_bus::typed::service;

// Workspace uses
use ya_client_model::payment::*;
use ya_core_model::payment::local::{
    DriverName, NetworkName, PaymentDriverStatus, PaymentDriverStatusError,
    BUS_ID as PAYMENT_BUS_ID,
};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

// Local uses
use crate::dao::*;
use crate::utils::*;

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .route("/payments", get().to(get_payments))
        .route("/payments/status", get().to(payment_status))
        .route("/payments/{payment_id}", get().to(get_payment))
}

async fn get_payments(
    db: Data<DbExecutor>,
    query: Query<params::DriverNetworkParams>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let timeout_secs = query
        .event_params
        .timeout
        .unwrap_or(params::DEFAULT_EVENT_TIMEOUT);
    let after_timestamp = query.event_params.after_timestamp.map(|d| d.naive_utc());
    let network = match query
        .network
        .as_ref()
        .map(|n| NetworkName::from_str(n.as_str()))
        .map_or(Ok(None), |v| v.map(Some))
    {
        Ok(network) => network,
        Err(e) => return response::server_error(&e),
    };
    let driver = match query
        .driver
        .as_ref()
        .map(|d| DriverName::from_str(d.as_str()))
        .map_or(Ok(None), |v| v.map(Some))
    {
        Ok(driver) => driver,
        Err(e) => return response::server_error(&e),
    };
    let max_events = query.event_params.max_events;
    let app_session_id = &query.event_params.app_session_id;

    let dao: PaymentDao = db.as_dao();
    let getter = || async {
        dao.get_for_node_id(
            node_id,
            after_timestamp,
            max_events,
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

async fn payment_status(
    db: Data<DbExecutor>,
    query: Query<params::DriverStatusParams>,
    id: Identity,
) -> HttpResponse {
    let result = service(PAYMENT_BUS_ID)
        .call(PaymentDriverStatus {
            driver: query.driver.clone(),
            network: query.network.clone(),
        })
        .await;

    let response = match result {
        Ok(resp) => resp,
        Err(e) => return response::server_error(&e),
    };

    let status_props = match response {
        Ok(props) => props,
        Err(PaymentDriverStatusError::NoDriver) => return response::bad_request(&"No such driver"),
        Err(PaymentDriverStatusError::Internal(e)) => return response::server_error(&e),
    };

    response::ok(status_props)
}
