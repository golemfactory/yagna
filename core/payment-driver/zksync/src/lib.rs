#![allow(clippy::useless_vec)]
#![allow(clippy::arc_with_non_send_sync)]

/*
    Payment driver for yagna using zksync.

    This file only contains constants and imports.
*/

// Public
pub const DRIVER_NAME: &str = "zksync";

pub const DEFAULT_NETWORK: &str = "rinkeby";
pub const DEFAULT_TOKEN: &str = "tGLM";
pub const DEFAULT_PLATFORM: &str = "zksync-rinkeby-tglm";

pub const MAINNET_NETWORK: &str = "mainnet";
pub const MAINNET_TOKEN: &str = "GLM";
pub const MAINNET_PLATFORM: &str = "zksync-mainnet-glm";

pub use service::ZksyncService as PaymentDriverService;

// Private
#[macro_use]
extern crate log;

mod dao;
mod driver;
mod network;
mod service;
pub mod zksync;
