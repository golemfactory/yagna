pub mod dummy;
pub mod ident;
pub mod resolver;

pub use crate::middleware::auth::ident::Identity;
pub use crate::middleware::auth::resolver::AppKeyCache;

use crate::middleware::allow_all_cors::add_full_allow_headers;
use actix_service::{Service, Transform};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::error::{Error, InternalError, ParseError};
use actix_web::{web, HttpMessage};
use actix_web_httpauth::headers::authorization::{Bearer, Scheme};
use futures::future::{ok, Future, Ready};
use serde::Deserialize;
use std::cell::RefCell;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

pub struct Auth {
    pub(crate) cache: AppKeyCache,
    pub(crate) allow_cors_on_authentication_failure: bool,
}

impl Auth {
    pub fn new(cache: AppKeyCache, allow_cors_on_authentication_failure: bool) -> Auth {
        Auth {
            cache,
            allow_cors_on_authentication_failure,
        }
    }
}

impl<S, B> Transform<S, ServiceRequest> for Auth
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = AuthMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(AuthMiddleware {
            service: Rc::new(RefCell::new(service)),
            cache: self.cache.clone(),
            allow_cors_on_authentication_failure: self.allow_cors_on_authentication_failure,
        })
    }
}

pub struct AuthMiddleware<S> {
    service: Rc<RefCell<S>>,
    cache: AppKeyCache,
    allow_cors_on_authentication_failure: bool,
}

impl<S, B> Service<ServiceRequest> for AuthMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.borrow_mut().poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct QueryAuth {
            auth_token: String,
        }

        let header = parse_auth::<Bearer, _>(&req)
            .ok()
            .map(|b| b.token().to_string())
            .or_else(|| {
                web::Query::<QueryAuth>::from_query(req.query_string())
                    .ok()
                    .map(|q| q.into_inner().auth_token)
            });

        let cache = self.cache.clone();
        let service = self.service.clone();

        let allowed_uris = vec!["/metrics-api", "/version", "/dashboard"];

        for uri in allowed_uris {
            if req.uri().to_string().starts_with(uri) {
                log::debug!("skipping authorization for uri={}", req.uri());
                return Box::pin(service.borrow_mut().call(req));
            }
        }

        let allow_cors_on_failure = self.allow_cors_on_authentication_failure;
        Box::pin(async move {
            match header {
                Some(key) => match cache.get_appkey(&key) {
                    Some(app_key) => {
                        req.extensions_mut().insert(Identity::from(app_key));
                        let fut = { service.borrow_mut().call(req) };
                        Ok(fut.await?)
                    }
                    None => {
                        log::debug!(
                            "{} {} Invalid application key: {key}",
                            req.method(),
                            req.path(),
                        );

                        let mut res = actix_web::HttpResponse::Unauthorized().finish();
                        if allow_cors_on_failure {
                            add_full_allow_headers(res.headers_mut());
                        }

                        Err(actix_web::Error::from(InternalError::from_response(
                            "Invalid application key",
                            res,
                        )))
                    }
                },
                None => {
                    log::debug!("Missing application key");
                    let mut res = actix_web::HttpResponse::Unauthorized().finish();

                    if allow_cors_on_failure {
                        add_full_allow_headers(res.headers_mut());
                    }

                    Err(actix_web::Error::from(InternalError::from_response(
                        "Missing application key",
                        res,
                    )))
                }
            }
        })
    }
}

pub(crate) fn parse_auth<S: Scheme, T: HttpMessage>(msg: &T) -> Result<S, ParseError> {
    let header = msg
        .headers()
        .get(actix_web::http::header::AUTHORIZATION)
        .ok_or(ParseError::Header)?;
    S::parse(header).map_err(|_| ParseError::Header)
}
