use std::net::SocketAddr;

pub mod middleware;
pub mod scope;

pub const YAGNA_API_URL_ENV_VAR: &str = "YAGNA_API_URL";
pub const DEFAULT_YAGNA_API_URL: &str = "http://127.0.0.1:7465";

pub fn rest_api_addr() -> SocketAddr {
    rest_api_url().parse().unwrap()
}

pub fn rest_api_url() -> String {
    std::env::var(YAGNA_API_URL_ENV_VAR).unwrap_or(DEFAULT_YAGNA_API_URL.into())
}
