mod message;
mod network;
mod requestor;
mod service;
mod tunneling;

pub use self::service::VpnService;

pub type Result<T> = std::result::Result<T, ya_utils_networking::vpn::Error>;
