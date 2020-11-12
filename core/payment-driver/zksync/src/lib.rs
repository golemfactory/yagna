/*
    Payment driver for yagna using zksync.

    This file only contains constants and imports.
*/

// Public
pub const PLATFORM_NAME: &'static str = "ZK-NGNT";
pub const DRIVER_NAME: &'static str = "zksync";
pub const ZKSYNC_TOKEN_NAME: &'static str = "GNT";

pub use service::ZksyncService as PaymentDriverService;

// Private
#[macro_use]
extern crate log;

mod dao;
mod driver;
mod service;
mod zksync;
