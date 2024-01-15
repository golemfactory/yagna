use actix_web::web::{self, Data};
use actix_web::Scope;
use ya_client_model::payment::PAYMENT_API_PATH;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::scope::ExtendableScope;

mod accounts;
pub mod allocations;
mod debit_notes;
mod invoices;
mod payments;

mod guard;

pub fn api_scope(scope: Scope) -> Scope {
    scope
        .app_data(web::Data::new(guard::AgreementLock::arc()))
        .extend(accounts::register_endpoints)
        .extend(allocations::register_endpoints)
        .extend(debit_notes::register_endpoints)
        .extend(invoices::register_endpoints)
        .extend(payments::register_endpoints)
}

pub fn web_scope(db: &DbExecutor) -> Scope {
    Scope::new(PAYMENT_API_PATH)
        .app_data(Data::new(db.clone()))
        .service(api_scope(Scope::new("")))
    // TODO: TEST
    // Scope::new(PAYMENT_API_PATH).extend(api_scope).app_data(Data::new(db.clone()))
}
