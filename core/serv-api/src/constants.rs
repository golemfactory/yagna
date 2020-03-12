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
