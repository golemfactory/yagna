/*
    Payment driver for yagna using zksync.

    This file only contains constants and imports.
*/

// Public
pub use config::{DriverConfig, GLMSYNC_CONFIG, ZKSYNC_CONFIG};
pub use service::ZksyncService as PaymentDriverService;

// Private
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;

mod config;
mod dao;
mod driver;
mod service;
pub mod zksync;
