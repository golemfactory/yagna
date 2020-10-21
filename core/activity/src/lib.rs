/// Yagna Activity Service

#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;

mod common;
mod dao;
pub mod db;

mod api;
mod cli;
mod error;
mod provider;
mod requestor;
pub mod service;

pub type Result<T> = std::result::Result<T, error::Error>;

pub use ya_client_model::activity::ACTIVITY_API_PATH;
