/// Yagna Market service

#[macro_use]
extern crate diesel;

#[macro_use]
extern crate ya_service_bus;

pub mod dao;
pub mod db;
pub mod error;
pub mod service;

pub use error::Error;
