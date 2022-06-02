pub use crate::middleware::auth::ident::Identity;

use actix_service::{Service, Transform};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::error::Error;
use actix_web::HttpMessage;
use futures::future::{ok, Ready};
use std::task::{Context, Poll};

pub struct DummyAuth {
    identity: Identity,
}

impl DummyAuth {
    pub fn new(identity: Identity) -> Self {
        Self { identity }
    }
}

impl<'s, S, B> Transform<S, ServiceRequest> for DummyAuth
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = DummyAuthMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(DummyAuthMiddleware {
            service,
            identity: self.identity.clone(),
        })
    }
}

pub struct DummyAuthMiddleware<S> {
    service: S,
    identity: Identity,
}

impl<S, B> Service<ServiceRequest> for DummyAuthMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = S::Future;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        req.extensions_mut().insert(self.identity.clone());
        self.service.call(req)
    }
}
