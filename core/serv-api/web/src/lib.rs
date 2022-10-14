pub mod middleware;
pub mod scope;

pub use ya_client::web::{rest_api_url, DEFAULT_YAGNA_API_URL, YAGNA_API_URL_ENV_VAR};

pub fn rest_api_addr() -> String {
    rest_api_host_port(rest_api_url())
}

pub fn rest_api_host_port(api_url: url::Url) -> String {
    let host = api_url
        .host()
        .unwrap_or_else(|| panic!("invalid API URL - no host: {}", api_url))
        .to_string();
    let port = api_url
        .port_or_known_default()
        .unwrap_or_else(|| panic!("invalid API URL - no port: {}", api_url));

    format!("{}:{}", host, port)
}
