/*
    Payment driver for yagna using erc20.

    This file only contains constants and imports.
*/

// Public
pub const DRIVER_NAME: &'static str = "erc20";

pub const RINKEBY_NETWORK: &'static str = "rinkeby";
pub const RINKEBY_TOKEN: &'static str = "tGLM";
pub const RINKEBY_PLATFORM: &'static str = "erc20-rinkeby-tglm";
pub const RINKEBY_CURRENCY_SHORT: &'static str = "tETH";
pub const RINKEBY_CURRENCY_LONG: &'static str = "Rinkeby Ether";

pub const GOERLI_NETWORK: &'static str = "goerli";
pub const GOERLI_TOKEN: &'static str = "tGLM";
pub const GOERLI_PLATFORM: &'static str = "erc20-goerli-tglm";
pub const GOERLI_CURRENCY_SHORT: &'static str = "tETH";
pub const GOERLI_CURRENCY_LONG: &'static str = "Goerli Ether";

pub const MUMBAI_NETWORK: &'static str = "mumbai";
pub const MUMBAI_TOKEN: &'static str = "tGLM";
pub const MUMBAI_PLATFORM: &'static str = "erc20-mumbai-tglm";
pub const MUMBAI_CURRENCY_SHORT: &'static str = "tMATIC";
pub const MUMBAI_CURRENCY_LONG: &'static str = "Test MATIC";

pub const MAINNET_NETWORK: &'static str = "mainnet";
pub const MAINNET_TOKEN: &'static str = "GLM";
pub const MAINNET_PLATFORM: &'static str = "erc20-mainnet-glm";
pub const MAINNET_CURRENCY_SHORT: &'static str = "ETH";
pub const MAINNET_CURRENCY_LONG: &'static str = "Ether";

pub const POLYGON_MAINNET_NETWORK: &'static str = "polygon";
pub const POLYGON_MAINNET_TOKEN: &'static str = "GLM";
pub const POLYGON_MAINNET_PLATFORM: &'static str = "erc20-polygon-glm";
pub const POLYGON_MAINNET_CURRENCY_SHORT: &'static str = "MATIC";
pub const POLYGON_MAINNET_CURRENCY_LONG: &'static str = "Polygon";

pub use service::Erc20Service as PaymentDriverService;

// Private
#[macro_use]
extern crate log;

mod dao;
mod driver;
pub mod erc20;
mod network;
mod service;
