/// Yagna Activity Service

#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;

pub use ya_client_model::activity::ACTIVITY_API_PATH;

mod common;
mod dao;
pub mod db;

mod api;
mod cli;
mod error;
mod http_proxy;
mod provider;
mod requestor;
pub mod service;
mod tracker;

pub type Result<T> = std::result::Result<T, error::Error>;
pub use self::tracker::TrackerRef;
