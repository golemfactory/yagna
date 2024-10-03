use actix_web::http::header::HeaderMap;
use actix_web::http::header::HeaderName;
use actix_web::http::header::HeaderValue;
use std::str::FromStr;
use structopt::lazy_static::lazy_static;

#[rustfmt::skip]
fn get_full_permissive_headers() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Access-Control-Allow-Origin", "*"),
        ("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS"),
        ("Access-Control-Allow-Headers", "Content-Type, Authorization"),
        ("Access-Control-Allow-Credentials", "true"),
        ("Access-Control-Max-Age", "3600"),
    ]
}

pub fn add_full_allow_headers(header_map: &mut HeaderMap) {
    lazy_static! {
        static ref FULL_PERMISIVE_HEADERS: Vec<(&'static str, &'static str)> =
            get_full_permissive_headers();
    }
    for (header_name, header_value) in FULL_PERMISIVE_HEADERS.iter() {
        header_map.insert(
            HeaderName::from_str(header_name).unwrap(),
            HeaderValue::from_str(header_value).unwrap(),
        );
    }
}
