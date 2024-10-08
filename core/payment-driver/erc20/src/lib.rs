/*
    Payment driver for yagna using erc20.

    This file only contains constants and imports.
*/

// Public
pub const DRIVER_NAME: &str = "erc20";

pub const RINKEBY_NETWORK: &str = "rinkeby";
pub const RINKEBY_TOKEN: &str = "tGLM";
pub const RINKEBY_PLATFORM: &str = "erc20-rinkeby-tglm";
pub const RINKEBY_CURRENCY_SHORT: &str = "tETH";
pub const RINKEBY_CURRENCY_LONG: &str = "Rinkeby Ether";

pub const GOERLI_NETWORK: &str = "goerli";
pub const GOERLI_TOKEN: &str = "tGLM";
pub const GOERLI_PLATFORM: &str = "erc20-goerli-tglm";
pub const GOERLI_CURRENCY_SHORT: &str = "tETH";
pub const GOERLI_CURRENCY_LONG: &str = "Goerli Ether";

pub const HOLESKY_NETWORK: &str = "holesky";
pub const HOLESKY_TOKEN: &str = "tGLM";
pub const HOLESKY_PLATFORM: &str = "erc20-holesky-tglm";
pub const HOLESKY_CURRENCY_SHORT: &str = "tETH";
pub const HOLESKY_CURRENCY_LONG: &str = "Holesky Ether";

pub const MUMBAI_NETWORK: &str = "mumbai";
pub const MUMBAI_TOKEN: &str = "tGLM";
pub const MUMBAI_PLATFORM: &str = "erc20-mumbai-tglm";
pub const MUMBAI_CURRENCY_SHORT: &str = "POL";
pub const MUMBAI_CURRENCY_LONG: &str = "Test POL";

pub const AMOY_NETWORK: &str = "amoy";
pub const AMOY_TOKEN: &str = "tGLM";
pub const AMOY_PLATFORM: &str = "erc20-amoy-tglm";
pub const AMOY_CURRENCY_SHORT: &str = "POL";
pub const AMOY_CURRENCY_LONG: &str = "Test POL";

pub const SEPOLIA_NETWORK: &str = "sepolia";
pub const SEPOLIA_TOKEN: &str = "tGLM";
pub const SEPOLIA_PLATFORM: &str = "erc20-sepolia-tglm";
pub const SEPOLIA_CURRENCY_SHORT: &str = "tETH";
pub const SEPOLIA_CURRENCY_LONG: &str = "Sepolia Ether";

pub const MAINNET_NETWORK: &str = "mainnet";
pub const MAINNET_TOKEN: &str = "GLM";
pub const MAINNET_PLATFORM: &str = "erc20-mainnet-glm";
pub const MAINNET_CURRENCY_SHORT: &str = "ETH";
pub const MAINNET_CURRENCY_LONG: &str = "Ether";

pub const POLYGON_MAINNET_NETWORK: &str = "polygon";
pub const POLYGON_MAINNET_TOKEN: &str = "GLM";
pub const POLYGON_MAINNET_PLATFORM: &str = "erc20-polygon-glm";
pub const POLYGON_MAINNET_CURRENCY_SHORT: &str = "POL";
pub const POLYGON_MAINNET_CURRENCY_LONG: &str = "Polygon";

pub use service::Erc20Service as PaymentDriverService;

// Private
#[macro_use]
extern crate log;

mod dao;
mod driver;
pub mod erc20;
mod network;
mod service;
mod signer;
