/*
    Payment driver for yagna using zksync.

    This file only contains constants and imports.
*/

// Public
pub const DRIVER_NAME: &'static str = "zksync";
pub const ZKSYNC_TOKEN_NAME: &'static str = "GNT";

pub(crate) const MAINNET_NAME: &str = "mainnet";
pub(crate) const MAINNET_TOKEN: &str = "GLM";
pub(crate) const RINKEBY_NAME: &str = "rinkeby";
pub(crate) const RINKEBY_TOKEN: &str = "tGLM";

pub const DEFAULT_NETWORK: &'static str = RINKEBY_NAME;
pub const DEFAULT_TOKEN: &'static str = RINKEBY_TOKEN;
pub const DEFAULT_PLATFORM: &'static str = "zksync-rinkeby-tglm"; // TODO: remove

pub use service::ZksyncService as PaymentDriverService;

// Private
#[macro_use]
extern crate log;

mod dao;
mod driver;
mod service;
pub mod zksync;
