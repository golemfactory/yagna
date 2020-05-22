use actix_web::Scope;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use ya_client_model::payment::PAYMENT_API_PATH;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::scope::ExtendableScope;

mod provider;
mod requestor;

pub fn provider_scope() -> Scope {
    Scope::new("/provider").extend(provider::register_endpoints)
}

pub fn requestor_scope() -> Scope {
    Scope::new("/requestor").extend(requestor::register_endpoints)
}

pub fn web_scope(db: &DbExecutor) -> Scope {
    Scope::new(PAYMENT_API_PATH)
        .data(db.clone())
        .service(provider_scope())
        .service(requestor_scope())
}

pub const DEFAULT_ACK_TIMEOUT: f64 = 60.0; // seconds
pub const DEFAULT_EVENT_TIMEOUT: f64 = 0.0; // seconds

#[inline(always)]
pub(crate) fn default_ack_timeout() -> f64 {
    DEFAULT_ACK_TIMEOUT
}

#[inline(always)]
pub(crate) fn default_event_timeout() -> f64 {
    DEFAULT_EVENT_TIMEOUT
}

#[derive(Deserialize)]
pub struct DebitNoteId {
    pub debit_note_id: String,
}

#[derive(Deserialize)]
pub struct InvoiceId {
    pub invoice_id: String,
}

#[derive(Deserialize)]
pub struct AllocationId {
    pub allocation_id: String,
}

#[derive(Deserialize)]
pub struct PaymentId {
    pub payment_id: String,
}

#[derive(Deserialize)]
pub struct Timeout {
    #[serde(default = "default_ack_timeout")]
    pub timeout: f64,
}

#[derive(Deserialize)]
pub struct EventParams {
    #[serde(default = "default_event_timeout")]
    pub timeout: f64,
    #[serde(rename = "laterThan")]
    pub later_than: Option<DateTime<Utc>>,
}
