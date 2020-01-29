use actix_web::Scope;
use serde::Deserialize;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::scope::ExtendableScope;

mod provider;
mod requestor;

pub fn web_scope(db: &DbExecutor) -> Scope {
    Scope::new(crate::PAYMENT_API)
        .data(db.clone())
        .service(Scope::new("/provider").extend(provider::register_endpoints))
        .service(Scope::new("/requestor").extend(requestor::register_endpoints))
}

pub const DEFAULT_ACK_TIMEOUT: u32 = 60; // seconds
pub const DEFAULT_EVENT_TIMEOUT: u32 = 0; // seconds

#[inline(always)]
pub(crate) fn default_ack_timeout() -> Option<u32> {
    Some(DEFAULT_ACK_TIMEOUT)
}

#[inline(always)]
pub(crate) fn default_event_timeout() -> Option<u32> {
    Some(DEFAULT_EVENT_TIMEOUT)
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
    pub payment_id: String,
}

#[derive(Deserialize)]
pub struct PaymentId {
    pub payment_id: String,
}

#[derive(Deserialize)]
pub struct Timeout {
    #[serde(default = "default_ack_timeout")]
    pub timeout: Option<u32>,
}

#[derive(Deserialize)]
pub struct EventParams {
    #[serde(default = "default_event_timeout")]
    pub timeout: Option<u32>,
    #[serde(rename = "laterThan")]
    pub later_than: Option<String>, // FIXME: change to chrono::DateTime
}
