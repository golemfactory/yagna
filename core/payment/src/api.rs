use actix_web::Scope;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer};
use ya_client_model::payment::PAYMENT_API_PATH;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::scope::ExtendableScope;

mod accounts;
mod allocations;
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
    Scope::new(PAYMENT_API_PATH)
        .data(db.clone())
        .service(api_scope(Scope::new("")))
    // TODO: TEST
    // Scope::new(PAYMENT_API_PATH).extend(api_scope).data(db.clone())
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

#[derive(Deserialize)]
pub struct FilterParams {
    #[serde(rename = "maxItems", default)]
    pub max_items: Option<u32>,
    #[serde(rename = "afterTimestamp", default)]
    pub after_timestamp: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
pub struct AllocationIds {
    #[serde(
        rename = "allocationIds",
        deserialize_with = "deserialize_comma_separated"
    )]
    pub allocation_ids: Vec<String>,
}

fn deserialize_comma_separated<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: &str = Deserialize::deserialize(deserializer)?;
    Ok(s.split(",").map(str::to_string).collect())
}
