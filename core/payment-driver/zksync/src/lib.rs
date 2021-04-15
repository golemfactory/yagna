/*
    Payment driver for yagna using zksync.

    This file only contains constants and imports.
*/

// Public
pub const DRIVER_NAME: &'static str = "zksync";

pub const DEFAULT_NETWORK: &'static str = "rinkeby";
pub const DEFAULT_TOKEN: &'static str = "tGLM";
pub const DEFAULT_PLATFORM: &'static str = "zksync-rinkeby-tglm";

pub const MAINNET_NETWORK: &'static str = "mainnet";
pub const MAINNET_TOKEN: &'static str = "GLM";
pub const MAINNET_PLATFORM: &'static str = "zksync-mainnet-glm";

pub use service::ZksyncService as PaymentDriverService;

// Private
#[macro_use]
extern crate log;

mod dao;
mod driver;
mod network;
mod service;
pub mod zksync;
