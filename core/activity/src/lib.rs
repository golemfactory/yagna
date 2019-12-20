#[macro_use]
extern crate diesel;

#[macro_use]
pub(crate) mod common;
pub(crate) mod dao;
pub(crate) mod db;

pub mod error;
pub mod provider;
pub mod requestor;
pub mod timeout;

pub type Result<T> = std::result::Result<T, error::Error>;

pub const NET_SERVICE_ID: &str = "net";
pub const ACTIVITY_SERVICE_ID: &str = "activity";
pub const ACTIVITY_SERVICE_VERSION: u8 = 1;

pub trait GsbApi {
    fn bind(api: &'static Self);
}

pub trait RestfulApi {
    fn web_scope(api: &'static Self) -> actix_web::Scope;
}
