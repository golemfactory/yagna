mod device;
mod interface;
mod message;
mod network;
mod requestor;
mod service;

pub use self::service::VpnService;
use ya_utils_networking::vpn::Error;

pub type Result<T> = std::result::Result<T, Error>;
