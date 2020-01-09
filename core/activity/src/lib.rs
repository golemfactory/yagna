#[macro_use]
extern crate diesel;

#[macro_use]
pub(crate) mod macros;
pub(crate) mod common;
pub(crate) mod dao;

pub mod error;
pub mod provider;
pub mod requestor;
pub mod timeout;

pub type Result<T> = std::result::Result<T, error::Error>;

pub const NET_SERVICE_ID: &str = "net";
pub const ACTIVITY_SERVICE_ID: &str = "activity";

lazy_static::lazy_static! {
    pub static ref ACTIVITY_SERVICE_URI: String = format!("/{}/v{}", ACTIVITY_SERVICE_ID, 1);
}
