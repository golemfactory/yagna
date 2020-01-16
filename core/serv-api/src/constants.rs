use std::env;

// Local service basics
lazy_static::lazy_static! {
pub static ref YAGNA_HOST: String = env::var("YAGNA_HOST").unwrap_or("127.0.0.1".into());
pub static ref YAGNA_BUS_PORT: String = env::var("YAGNA_BUS_PORT").unwrap_or("7464".into());
pub static ref YAGNA_HTTP_PORT: String = env::var("YAGNA_HTTP_PORT").unwrap_or("7465".into());

pub static ref YAGNA_BUS_ADDR_STR: String = format!("{}:{}", *YAGNA_HOST, *YAGNA_BUS_PORT);
pub static ref YAGNA_BUS_ADDR: std::net::SocketAddr = YAGNA_BUS_ADDR_STR.parse().unwrap();
pub static ref YAGNA_HTTP_ADDR_STR: String = format!("{}:{}", *YAGNA_HOST, *YAGNA_HTTP_PORT);
pub static ref YAGNA_HTTP_ADDR: std::net::SocketAddr = YAGNA_HTTP_ADDR_STR.parse().unwrap();
}

// Bus service prefixes
pub const PRIVATE_SERVICE: &str = "/private";
pub const PUBLIC_SERVICE: &str = "/public";
pub const NET_SERVICE_ID: &str = "/net";

// Centralised network defaults
#[allow(dead_code)]
pub const CENTRAL_NET_HOST: &str = "10.30.10.202:7477"; // awokado

// services
pub const ACTIVITY_SERVICE_ID: &str = "/activity";
pub const APP_KEY_SERVICE_ID: &str = "/appkey";
pub const IDENTITY_SERVICE_ID: &str = "/identity";

// APIs
pub const MARKET_API: &str = "/market-api/v1";
pub const ACTIVITY_API: &str = "/activity-api/v1";
#[allow(dead_code)]
pub const PAYMENT_API: &str = "/payment-api/v1";
