pub mod dummy;
pub mod ident;
pub mod resolver;

pub use crate::middleware::auth::ident::Identity;

use crate::middleware::auth::resolver::AppKeyResolver;
use actix_service::{Service, Transform};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::error::{Error, ErrorUnauthorized};
use actix_web::{http::header::Header, HttpMessage};
use actix_web_httpauth::headers::authorization::{Authorization, Bearer};
use futures::future::{ok, Future, Ready};
use futures::lock::Mutex;
use std::cell::RefCell;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll};
use ya_service_api_cache::AutoResolveCache;

pub type Cache = AutoResolveCache<AppKeyResolver>;

pub struct Auth {
    cache: Arc<Mutex<Cache>>,
}

impl Default for Auth {
    fn default() -> Self {
        let cache = Arc::new(Mutex::new(Cache::default()));
        Auth { cache }
    }
}

impl<'s, S, B> Transform<S> for Auth
where
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Request = ServiceRequest;
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
    cache: Arc<Mutex<Cache>>,
}

impl<S, B> Service for AuthMiddleware<S>
where
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
{
    type Request = ServiceRequest;
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.borrow_mut().poll_ready(cx)
    }

    fn call(&mut self, req: ServiceRequest) -> Self::Future {
        let header = Authorization::<Bearer>::parse(&req)
            .ok()
            .map(|a| a.into_scheme().token().to_string());

        let cache = self.cache.clone();
        let service = self.service.clone();

        // TODO: remove this hack; possibly by enabling creation of arbitrary appkey from CLI
        if req.uri().to_string().starts_with("/metrics-api") {
            log::debug!("skipping authorization for uri={}", req.uri());
            return Box::pin(service.borrow_mut().call(req));
        }

        Box::pin(async move {
            match header {
                Some(key) => {
                    let cached = cache.lock().await.get(&key);
                    let resolved = match cached {
                        Some(opt) => opt,
                        None => cache.lock().await.resolve(&key).await,
                    };

                    match resolved {
                        Some(app_key) => {
                            req.extensions_mut().insert(Identity::from(app_key));
                            let fut = { service.borrow_mut().call(req) };
                            Ok(fut.await?)
                        }
                        None => {
                            log::debug!(
                                "{} {} Invalid application key: {}",
                                req.method(),
                                req.path(),
                                key
                            );
                            Err(ErrorUnauthorized("Invalid application key"))
                        }
                    }
                }
                None => {
                    log::debug!("Missing application key");
                    Err(ErrorUnauthorized("Missing application key"))
                }
            }
        })
    }
}
