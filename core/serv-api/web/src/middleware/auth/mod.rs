pub mod dummy;
pub mod ident;
pub mod resolver;
mod sanitize;

pub use crate::middleware::auth::ident::{Admin, Identity, Role};
pub use crate::middleware::auth::resolver::{AdminCredential, AppKeyCache};
pub use sanitize::sanitize_query_auth;

use actix_service::{Service, Transform};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::error::{Error, ErrorUnauthorized, ParseError};
use actix_web::http::header;
use actix_web::HttpMessage;
use actix_web_httpauth::headers::authorization::{Bearer, Scheme};
use futures::future::{ok, Future, Ready};
use sanitize::QueryCredential;
use std::cell::RefCell;
use std::net::{IpAddr, SocketAddr};
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

enum Credential {
    Bearer(String),
    Query(String),
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

    fn call(&self, mut req: ServiceRequest) -> Self::Future {
        // Production installs an outer sanitizer before the access logger.
        // Keep this fallback so Auth remains safe and query-compatible when
        // embedded without that outer wrapper.
        if let Err(error) = sanitize::sanitize_request(&mut req) {
            return Box::pin(async move { Err(error) });
        }

        // An Authorization header always takes precedence. In particular, a
        // malformed or invalid header must not fall back to a query credential.
        let query_credential = req.extensions_mut().remove::<QueryCredential>();
        let query_credential_was_supplied = query_credential.is_some();
        let has_authorization_header = req.headers().contains_key(header::AUTHORIZATION);
        let credential = if has_authorization_header {
            parse_auth::<Bearer, _>(&req)
                .ok()
                .map(|bearer| Credential::Bearer(bearer.token().to_string()))
        } else {
            match query_credential {
                Some(QueryCredential::Token(token)) => Some(Credential::Query(token)),
                Some(QueryCredential::Invalid) | None => None,
            }
        };
        let credential_was_supplied = has_authorization_header || query_credential_was_supplied;
        let peer_addr = req.peer_addr();

        let cache = self.cache.clone();
        let service = self.service.clone();

        let allowed_uris = ["/metrics-api", "/version/get", "/dashboard"];

        for uri in allowed_uris {
            if req.uri().to_string().starts_with(uri) {
                log::debug!("skipping authorization for uri={}", req.uri());
                return Box::pin(service.borrow_mut().call(req));
            }
        }

        Box::pin(async move {
            let principal = match credential {
                Some(Credential::Bearer(token)) => cache.resolve_bearer(&token),
                Some(Credential::Query(token)) => cache.resolve_query(&token),
                None => None,
            }
            .filter(|principal| principal_allowed_from_peer(principal, peer_addr));

            match principal {
                Some(principal) => {
                    req.extensions_mut().insert(principal);
                    let fut = { service.borrow_mut().call(req) };
                    Ok(fut.await?)
                }
                None if credential_was_supplied => {
                    log::debug!("{} {} Invalid application key", req.method(), req.path());
                    Err(ErrorUnauthorized("Invalid application key"))
                }
                None => {
                    log::debug!("Missing application key");
                    Err(ErrorUnauthorized("Missing application key"))
                }
            }
        })
    }
}

fn principal_allowed_from_peer(principal: &Identity, peer_addr: Option<SocketAddr>) -> bool {
    if principal.role != Role::Admin {
        return true;
    }

    peer_addr
        .map(|peer| match peer.ip() {
            IpAddr::V4(ip) => ip.is_loopback(),
            IpAddr::V6(ip) => {
                ip.is_loopback()
                    || ip
                        .to_ipv4_mapped()
                        .map(|mapped| mapped.is_loopback())
                        .unwrap_or(false)
            }
        })
        .unwrap_or(false)
}

pub(crate) fn parse_auth<S: Scheme, T: HttpMessage>(msg: &T) -> Result<S, ParseError> {
    let header = msg
        .headers()
        .get(header::AUTHORIZATION)
        .ok_or(ParseError::Header)?;
    S::parse(header).map_err(|_| ParseError::Header)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ya_client::model::NodeId;

    fn manager() -> Identity {
        Identity {
            identity: NodeId::default(),
            name: "manager".to_string(),
            subject: "manager".to_string(),
            role: Role::Manager,
        }
    }

    #[test]
    fn admin_principal_is_accepted_only_from_real_loopback_peer() {
        let admin = Identity::admin(NodeId::default());

        assert!(principal_allowed_from_peer(
            &admin,
            Some("127.0.0.1:1234".parse().unwrap())
        ));
        assert!(principal_allowed_from_peer(
            &admin,
            Some("[::1]:1234".parse().unwrap())
        ));
        assert!(principal_allowed_from_peer(
            &admin,
            Some("[::ffff:127.0.0.1]:1234".parse().unwrap())
        ));
        assert!(!principal_allowed_from_peer(
            &admin,
            Some("192.0.2.1:1234".parse().unwrap())
        ));
        assert!(!principal_allowed_from_peer(&admin, None));
    }

    #[test]
    fn manager_principal_is_not_restricted_to_loopback() {
        assert!(principal_allowed_from_peer(
            &manager(),
            Some("192.0.2.1:1234".parse().unwrap())
        ));
        assert!(principal_allowed_from_peer(&manager(), None));
    }
}
