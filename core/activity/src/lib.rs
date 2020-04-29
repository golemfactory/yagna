#[macro_use]
extern crate diesel;

mod common;
mod dao;

mod api;
mod error;
mod provider;
mod requestor;
pub mod service;

pub type Result<T> = std::result::Result<T, error::Error>;

pub use ya_client_model::activity::ACTIVITY_API_PATH;
