pub mod dummy;
pub mod ident;
pub mod resolver;

pub use crate::middleware::auth::ident::Identity;
pub use crate::middleware::auth::resolver::AppKeyCache;

use actix_service::{Service, Transform};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::error::{Error, ErrorUnauthorized, ParseError};
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
}

impl Auth {
    pub fn new(cache: AppKeyCache) -> Auth {
        Auth { cache }
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
        })
    }
}

pub struct AuthMiddleware<S> {
    service: Rc<RefCell<S>>,
    cache: AppKeyCache,
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
                if Some("websocket".as_bytes()) == req.headers().get("upgrade").map(AsRef::as_ref) {
                    web::Query::<QueryAuth>::from_query(req.query_string())
                        .ok()
                        .map(|q| q.into_inner().auth_token)
                } else {
                    None
                }
            });

        let cache = self.cache.clone();
        let service = self.service.clone();

        // TODO: remove this hack; possibly by enabling creation of arbitrary appkey from CLI
        if req.uri().to_string().starts_with("/metrics-api")
            || req.uri().to_string().starts_with("/version")
        {
            log::debug!("skipping authorization for uri={}", req.uri());
            return Box::pin(service.borrow_mut().call(req));
        }

        Box::pin(async move {
            let header = if let Some(key) = header {
                Some(key)
            } else {
                //use lazy static to not call env var on every request
                lazy_static::lazy_static! {
                    static ref DISABLE_APPKEY_SECURITY : bool = std::env::var("YAGNA_DEV_DISABLE_APPKEY_SECURITY").map(|f|f == "1").unwrap_or(false);
                }
                if *DISABLE_APPKEY_SECURITY {
                    //dev path
                    log::warn!("AppKey security is disabled. Not for production!");
                    Some("no_security_appkey".to_string())
                } else {
                    //Normal path
                    None
                }
            };
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
                        Err(ErrorUnauthorized("Invalid application key"))
                    }
                },
                None => {
                    log::debug!("Missing application key");
                    Err(ErrorUnauthorized("Missing application key"))
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
