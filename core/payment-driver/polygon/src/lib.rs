/*
    Payment driver for yagna using erc20.

    This file only contains constants and imports.
*/

// Public
pub const DRIVER_NAME: &'static str = "polygon";

pub const MUMBAI_NETWORK: &'static str = "mumbai";
pub const MUMBAI_TOKEN: &'static str = "tGLM";
pub const MUMBAI_PLATFORM: &'static str = "polygon-mumbai-tglm";

pub const POLYGON_MAINNET_NETWORK: &'static str = "polygon";
pub const POLYGON_MAINNET_TOKEN: &'static str = "GLM";
pub const POLYGON_MAINNET_PLATFORM: &'static str = "polygon-polygon-glm";

pub use service::PolygonService as PaymentDriverService;

// Private
#[macro_use]
extern crate log;

mod dao;
mod driver;
pub mod polygon;
mod network;
mod service;
