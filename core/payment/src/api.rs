use std::sync::{Arc};
use actix_web::web::{Data, get};
use actix_web::{get, Scope};
use erc20_payment_lib::config;
use erc20_payment_lib::config::AdditionalOptions;
use erc20_payment_lib::db::create_sqlite_connection;
use erc20_payment_lib::runtime::SharedState;
use erc20_payment_lib::server::{runtime_web_scope, ServerData};
use erc20_payment_lib::setup::PaymentSetup;
use futures::executor;
use tokio::sync::Mutex;
use ya_client_model::payment::PAYMENT_API_PATH;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::scope::ExtendableScope;

mod accounts;
pub mod allocations;
mod debit_notes;
mod invoices;
mod payments;

pub fn api_scope(scope: Scope) -> Scope {
    scope
        .extend(accounts::register_endpoints)
        .extend(allocations::register_endpoints)
        .extend(debit_notes::register_endpoints)
        .extend(invoices::register_endpoints)
        .extend(payments::register_endpoints)
}

pub fn web_scope(db: &DbExecutor) -> Scope {
    println!("web_scope");

    // TODO: TEST
    // Scope::new(PAYMENT_API_PATH).extend(api_scope).app_data(Data::new(db.clone()))

    let private_keys = vec![];
    let receiver_accounts = vec![];
    let additional_options = AdditionalOptions {
        keep_running: true,
        generate_tx_only: false,
        skip_multi_contract_check: false,
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
    )
        .unwrap();

    let shared_state = SharedState {
        current_tx_info: Default::default(),
        faucet: None,
        inserted: 0,
        idling: false,
    };
    let db_filename = "db.sqlite";
    let conn = executor::block_on(create_sqlite_connection(Some(&db_filename), true)).unwrap();

    let server_data = Data::new(Box::new(ServerData {
        shared_state: Arc::new(Mutex::new(shared_state)),
        db_connection: Arc::new(Mutex::new(conn)),
        payment_setup: payment_setup,
    }));
    let erc20_scope = Scope::new("erc20");

    let erc20_scope = runtime_web_scope(erc20_scope, server_data, true, true, true);


    let payments_scope = Scope::new(PAYMENT_API_PATH)
        .app_data(Data::new(db.clone()))
        .service(api_scope(Scope::new("")));

    Scope::new("").service(erc20_scope).service(payments_scope)
}
