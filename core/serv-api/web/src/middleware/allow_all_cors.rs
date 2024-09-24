#![allow(clippy::new_without_default)]

use actix_service::{Service, Transform};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::error::InternalError;
use actix_web::http::header::HeaderMap;
use actix_web::http::header::HeaderName;
use actix_web::http::header::HeaderValue;
use futures::future::{ok, Ready};
use std::pin::Pin;
use std::rc::Rc;
use std::str::FromStr;
use std::task::{Context, Poll};
use structopt::lazy_static::lazy_static;

// Define Middleware Struct
pub struct AllowAllCors {
    _empty: u64,
}

impl AllowAllCors {
    pub fn new() -> Self {
        static ATOMIC_U64: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

        //display message only once thanks to atomic counter
        if ATOMIC_U64.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0 {
            let mut message =
                "Using AllowAllCors middleware: following headers will be added to all requests:\n"
                    .to_string();
            for (header_name, header_value) in get_full_permissive_headers().iter() {
                message += &format!("{}: {}\n", header_name, header_value);
            }
            log::info!("{}", message);
        }

        Self { _empty: 0 }
    }
}

// Middleware Implementation
impl<S, B> Transform<S, ServiceRequest> for AllowAllCors
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = actix_web::Error;
    type Transform = AllowAllCorsMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(AllowAllCorsMiddleware {
            service: Rc::new(service),
        })
    }
}

pub struct AllowAllCorsMiddleware<S> {
    service: Rc<S>,
}

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

impl<S, B> Service<ServiceRequest> for AllowAllCorsMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = actix_web::Error;
    type Future = Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&self, ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(ctx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let fut = self.service.call(req);

        Box::pin(async move {
            match fut.await {
                Ok(mut res) => {
                    log::debug!("Adding full allow headers");
                    add_full_allow_headers(res.headers_mut());
                    Ok(res)
                }
                Err(err) => {
                    log::debug!("Adding full allow headers to error response");
                    // Create an error response and add the "my-header"
                    let mut res = err.error_response();
                    add_full_allow_headers(res.headers_mut());

                    Err(actix_web::Error::from(InternalError::from_response(
                        err, res,
                    )))
                }
            }
        })
    }
}
