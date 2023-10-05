/*
    Payment driver for yagna using zksync.

    This file only contains constants and imports.
*/

// Public
pub const DRIVER_NAME: &str = "zksync";

pub const MAINNET_NETWORK: &str = "mainnet";
pub const MAINNET_TOKEN: &str = "GLM";
pub const MAINNET_PLATFORM: &str = "zksync-mainnet-glm";

pub const DEFAULT_NETWORK: &str = MAINNET_NETWORK;
pub const DEFAULT_TOKEN: &str = MAINNET_TOKEN;
pub const DEFAULT_PLATFORM: &str = MAINNET_PLATFORM;

pub use service::ZksyncService as PaymentDriverService;

// Private
#[macro_use]
extern crate log;

mod dao;
mod driver;
mod network;
mod service;
pub mod zksync;
