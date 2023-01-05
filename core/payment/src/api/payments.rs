use std::collections::BTreeMap;
// External crates
use actix_web::web::{get, Data, Path, Query};
use actix_web::{HttpResponse, Scope};
use std::str::FromStr;
use std::sync::Arc;
use erc20_payment_lib::config;
use erc20_payment_lib::config::AdditionalOptions;
use erc20_payment_lib::db::create_sqlite_connection;
use erc20_payment_lib::runtime::{SharedState, SharedInfoTx};
use erc20_payment_lib::server::{config_endpoint, ServerData};
use erc20_payment_lib::setup::PaymentSetup;
use futures::executor;
use tokio::sync::Mutex;

// Workspace uses
use ya_client_model::payment::*;
use ya_core_model::payment::local::{DriverName, NetworkName};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

// Local uses
use crate::dao::*;
use crate::utils::*;

pub fn register_endpoints(scope: Scope) -> Scope {
    let private_keys = vec![];
    let receiver_accounts = vec![];
    let additional_options = AdditionalOptions {
        keep_running: true,
        generate_tx_only: false,
        skip_multi_contract_check: false
    };
    let config = config::Config::load("config-payments.toml").unwrap();


    let payment_setup = PaymentSetup::new(
        &config,
        private_keys,
        receiver_accounts,
        !additional_options.keep_running,
        additional_options.generate_tx_only,
        additional_options.skip_multi_contract_check,
        config.engine.service_sleep,
        config.engine.process_sleep,
        config.engine.automatic_recover,
    ).unwrap();

    let shared_state = SharedState{
        current_tx_info: Default::default(),
        faucet: None,
        inserted: 0,
        idling: false
    };
    let db_filename = "db.sqlite";
    let conn = executor::block_on(create_sqlite_connection(Some(&db_filename), true)).unwrap();

    let server_data = ServerData {
        shared_state: Arc::new(Mutex::new(shared_state)),
        db_connection: Arc::new(Mutex::new(conn)),
        payment_setup: payment_setup
    };

    scope
        .route("/payments", get().to(get_payments))
        .route("/payments/{payment_id}", get().to(get_payment))
        .app_data(Data::new(server_data))
        .route("/payments/erc20/config", get().to(config_endpoint))
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
