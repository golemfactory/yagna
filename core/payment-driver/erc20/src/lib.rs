/*
    Payment driver for yagna using erc20.

    This file only contains constants and imports.
*/

// Public
pub const DRIVER_NAME: &'static str = "erc20";

pub const DEFAULT_NETWORK: &'static str = "rinkeby";
pub const DEFAULT_TOKEN: &'static str = "tGLM";
pub const DEFAULT_PLATFORM: &'static str = "erc20-rinkeby-tglm";

pub const MAINNET_NETWORK: &'static str = "mainnet";
pub const MAINNET_TOKEN: &'static str = "GLM";
pub const MAINNET_PLATFORM: &'static str = "erc20-mainnet-glm";

pub use service::Erc20Service as PaymentDriverService;

// Private
#[macro_use]
extern crate log;

mod dao;
mod driver;
pub mod erc20;
mod network;
mod service;
