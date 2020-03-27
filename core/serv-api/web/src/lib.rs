use std::net::SocketAddr;
use url::Url;

pub mod middleware;
pub mod scope;

pub const YAGNA_API_URL_ENV_VAR: &str = "YAGNA_API_URL";
pub const DEFAULT_YAGNA_API_URL: &str = "http://127.0.0.1:7465";

pub fn rest_api_addr() -> SocketAddr {
    let api_url = Url::parse(&rest_api_url()).expect("provide API URL in format http://<ip:port>");

    let ip_addr = api_url
        .host_str()
        .expect("need IP address for API URL")
        .parse()
        .expect("only IP address supported for API URL");

    let port = api_url
        .port()
        .unwrap_or_else(|| Url::parse(DEFAULT_YAGNA_API_URL).unwrap().port().unwrap());

    SocketAddr::new(ip_addr, port)
}

pub fn rest_api_url() -> String {
    std::env::var(YAGNA_API_URL_ENV_VAR).unwrap_or(DEFAULT_YAGNA_API_URL.into())
}
