#[macro_use]
extern crate diesel;
extern crate ya_service_api_web;
#[macro_use]
extern crate ya_service_bus;

pub(crate) mod common;
pub(crate) mod dao;
#[macro_use]
pub(crate) mod macros;

pub mod api;
pub mod error;
pub mod provider;
pub mod requestor;

pub type Result<T> = std::result::Result<T, error::Error>;

pub use ya_model::activity::ACTIVITY_API_PATH;
