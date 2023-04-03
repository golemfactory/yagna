#![allow(clippy::unit_arg)]
mod message;
mod network;
mod requestor;
mod service;

pub use self::service::VpnService;

pub type Result<T> = std::result::Result<T, ya_utils_networking::vpn::Error>;
