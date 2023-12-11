/*
    Payment driver for yagna using erc20next.

    This file only contains constants and imports.
*/

// Public
pub const DRIVER_NAME: &str = "erc20next";

pub const RINKEBY_NETWORK: &str = "rinkeby";
pub const RINKEBY_TOKEN: &str = "tGLM";
pub const RINKEBY_PLATFORM: &str = "erc20next-rinkeby-tglm";
pub const RINKEBY_CURRENCY_SHORT: &str = "tETH";
pub const RINKEBY_CURRENCY_LONG: &str = "Rinkeby Ether";

pub const GOERLI_NETWORK: &str = "goerli";
pub const GOERLI_TOKEN: &str = "tGLM";
pub const GOERLI_PLATFORM: &str = "erc20next-goerli-tglm";
pub const GOERLI_CURRENCY_SHORT: &str = "tETH";
pub const GOERLI_CURRENCY_LONG: &str = "Goerli Ether";

pub const HOLESKY_NETWORK: &str = "holesky";
pub const HOLESKY_TOKEN: &str = "tGLM";
pub const HOLESKY_PLATFORM: &str = "erc20next-holesky-tglm";
pub const HOLESKY_CURRENCY_SHORT: &str = "tETH";
pub const HOLESKY_CURRENCY_LONG: &str = "Holesky Ether";

pub const MUMBAI_NETWORK: &str = "mumbai";
pub const MUMBAI_TOKEN: &str = "tGLM";
pub const MUMBAI_PLATFORM: &str = "erc20next-mumbai-tglm";
pub const MUMBAI_CURRENCY_SHORT: &str = "tMATIC";
pub const MUMBAI_CURRENCY_LONG: &str = "Test MATIC";

pub const MAINNET_NETWORK: &str = "mainnet";
pub const MAINNET_TOKEN: &str = "GLM";
pub const MAINNET_PLATFORM: &str = "erc20next-mainnet-glm";
pub const MAINNET_CURRENCY_SHORT: &str = "ETH";
pub const MAINNET_CURRENCY_LONG: &str = "Ether";

pub const POLYGON_MAINNET_NETWORK: &str = "polygon";
pub const POLYGON_MAINNET_TOKEN: &str = "GLM";
pub const POLYGON_MAINNET_PLATFORM: &str = "erc20next-polygon-glm";
pub const POLYGON_MAINNET_CURRENCY_SHORT: &str = "MATIC";
pub const POLYGON_MAINNET_CURRENCY_LONG: &str = "Polygon";

pub use service::Erc20NextService as PaymentDriverService;

// Private
#[macro_use]
extern crate log;

mod dao;
mod driver;
pub mod erc20;
mod network;
mod service;
mod signer;
