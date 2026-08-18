use crate::service::{DropMessages, Service, StopService};
use actix::prelude::*;
use actix::{Actor, Addr, Context, Handler, Message};
use actix_http::ws::CloseReason;
use actix_web_actors::ws;
use lazy_static::lazy_static;
use std::{
    collections::HashMap,
    result::Result::{Err, Ok},
};
use thiserror::Error;

lazy_static! {
    pub(crate) static ref SERVICES: Addr<Services> = Services::default().start();
}

#[derive(Default)]
pub(crate) struct Services {
    services: HashMap<String, RegisteredService>,
}

struct RegisteredService {
    owner: String,
    service: Addr<Service>,
}

#[derive(Clone, Debug)]
pub(crate) struct Caller {
    pub subject: String,
    pub admin: bool,
}

impl Caller {
    fn can_access(&self, service: &RegisteredService) -> bool {
        self.admin || self.subject == service.owner
    }
}

impl Actor for Services {
    type Context = Context<Self>;
}

#[derive(Error, Debug)]
pub(crate) enum BindError {
    #[error("Duplicated service address prefix: {0}")]
    DuplicatedService(String),
    #[error("Invalid service address prefix: {0}")]
    InvalidService(String),
    #[error("Failed to bind service address prefix {0}: {1}")]
    BindFailed(String, String),
}

#[derive(Message, Debug)]
#[rtype(result = "Result<(), BindError>")]
pub(crate) struct Bind {
    pub components: Vec<String>,
    pub addr_prefix: String,
    pub owner: String,
}

impl Handler<Bind> for Services {
    type Result = <Bind as Message>::Result;

    fn handle(&mut self, msg: Bind, _ctx: &mut Self::Context) -> Self::Result {
        if msg.addr_prefix.is_empty() {
            return Err(BindError::InvalidService(
                "Cannot bind service. Empty prefix.".to_string(),
            ));
        }
        let addr = msg.addr_prefix.clone();
        let owner = msg.owner.clone();
        if self.services.contains_key(&addr) {
            return Err(BindError::DuplicatedService(addr));
        }
        let service = Service::from(msg).start();
        if let Err(error) =
            ya_service_bus::actix_rpc::try_bind_raw(&addr, service.clone().recipient())
        {
            service.do_send(StopService);
            return match error {
                ya_service_bus::Error::GsbAlreadyRegistered(_) => {
                    Err(BindError::DuplicatedService(addr))
                }
                error => Err(BindError::BindFailed(addr, error.to_string())),
            };
        }
        log::debug!("Created new service (addr: {}, owner: {})", addr, owner);
        self.services
            .insert(addr, RegisteredService { owner, service });
        Ok(())
    }
}

#[derive(Error, Debug)]
pub(crate) enum UnbindError {
    #[error("Service prefix not found: {0}")]
    ServiceNotFound(String),
    #[error("Invalid service address prefix: {0}")]
    InvalidService(String),
    #[error("Unbind failed: {0}")]
    UnbindFailed(String),
}

impl From<MailboxError> for UnbindError {
    fn from(err: MailboxError) -> Self {
        UnbindError::UnbindFailed(err.to_string())
    }
}

#[derive(Message, Debug)]
#[rtype(result = "Result<(), UnbindError>")]
pub(crate) struct Unbind {
    pub addr: String,
    pub caller: Caller,
}

impl Handler<Unbind> for Services {
    type Result = ResponseFuture<<Unbind as Message>::Result>;

    fn handle(&mut self, msg: Unbind, _ctx: &mut Self::Context) -> Self::Result {
        if msg.addr.is_empty() {
            return Box::pin(async {
                Err(UnbindError::InvalidService(
                    "Cannot unbind service. Empty prefix.".to_string(),
                ))
            });
        }
        let can_access = self
            .services
            .get(&msg.addr)
            .map(|service| msg.caller.can_access(service))
            .unwrap_or(false);
        if !can_access {
            return Box::pin(async move {
                Err(UnbindError::ServiceNotFound(format!(
                    "Cannot find service: {}",
                    msg.addr
                )))
            });
        }

        if !ya_service_bus::actix_rpc::unbind_raw(&msg.addr) {
            return Box::pin(async move {
                Err(UnbindError::UnbindFailed(format!(
                    "GSB endpoint is missing for service: {}",
                    msg.addr
                )))
            });
        }

        let registered = self
            .services
            .remove(&msg.addr)
            .expect("authorized service must still exist in actor state");
        Box::pin(async move {
            log::debug!(
                "Unbinding service: {} (caller: {})",
                msg.addr,
                msg.caller.subject
            );
            let error = CloseReason {
                code: ws::CloseCode::Normal,
                description: Some(format!("Unbinding service: {}", msg.addr)),
            };
            if let Err(error) = registered
                .service
                .send(DropMessages { reason: error })
                .await
            {
                log::warn!("Failed to close unbound GSB service actor: {error}");
            }
            Ok(())
        })
    }
}

#[derive(Error, Debug)]
pub(crate) enum FindError {
    #[error("Empty service address")]
    EmptyAddress,
    #[error("Service prefix not found: {0}")]
    ServiceNotFound(String),
}

#[derive(Message, Debug)]
#[rtype(result = "Result<Addr<Service>, FindError>")]
pub(crate) struct Find {
    pub addr: String,
    pub caller: Caller,
}

impl Handler<Find> for Services {
    type Result = <Find as Message>::Result;

    fn handle(&mut self, msg: Find, _ctx: &mut Self::Context) -> Self::Result {
        if msg.addr.is_empty() {
            return Err(FindError::EmptyAddress);
        }
        if let Some(service) = self.services.get(&msg.addr) {
            if msg.caller.can_access(service) {
                return Ok(service.service.clone());
            }
        }
        Err(FindError::ServiceNotFound(msg.addr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ya_service_bus::RpcRawCall;

    struct ExternalService;

    impl Actor for ExternalService {
        type Context = Context<Self>;
    }

    impl Handler<RpcRawCall> for ExternalService {
        type Result = Result<Vec<u8>, ya_service_bus::Error>;

        fn handle(&mut self, message: RpcRawCall, _ctx: &mut Self::Context) -> Self::Result {
            Ok(message.body)
        }
    }

    fn caller(subject: &str, admin: bool) -> Caller {
        Caller {
            subject: subject.to_string(),
            admin,
        }
    }

    #[actix_web::test]
    async fn foreign_owner_cannot_find_or_unbind_service() {
        let services = Services::default().start();
        let addr = format!("/local/gsb-api/test/{}", uuid::Uuid::new_v4());

        services
            .send(Bind {
                components: vec!["Call".to_string()],
                addr_prefix: addr.clone(),
                owner: "alice".to_string(),
            })
            .await
            .unwrap()
            .unwrap();

        let find_error = services
            .send(Find {
                addr: addr.clone(),
                caller: caller("bob", false),
            })
            .await
            .unwrap()
            .unwrap_err();
        assert!(matches!(find_error, FindError::ServiceNotFound(_)));

        let unbind_error = services
            .send(Unbind {
                addr: addr.clone(),
                caller: caller("bob", false),
            })
            .await
            .unwrap()
            .unwrap_err();
        assert!(matches!(unbind_error, UnbindError::ServiceNotFound(_)));

        services
            .send(Find {
                addr: addr.clone(),
                caller: caller("alice", false),
            })
            .await
            .unwrap()
            .expect("failed foreign operations must leave the binding intact");

        services
            .send(Unbind {
                addr,
                caller: caller("alice", false),
            })
            .await
            .unwrap()
            .unwrap();
    }

    #[actix_web::test]
    async fn admin_can_manage_foreign_service() {
        let services = Services::default().start();
        let addr = format!("/local/gsb-api/test/{}", uuid::Uuid::new_v4());

        services
            .send(Bind {
                components: vec!["Call".to_string()],
                addr_prefix: addr.clone(),
                owner: "alice".to_string(),
            })
            .await
            .unwrap()
            .unwrap();

        services
            .send(Find {
                addr: addr.clone(),
                caller: caller("administrator", true),
            })
            .await
            .unwrap()
            .unwrap();

        services
            .send(Unbind {
                addr,
                caller: caller("administrator", true),
            })
            .await
            .unwrap()
            .unwrap();
    }

    #[actix_web::test]
    async fn bind_rejects_address_registered_outside_the_http_api() {
        let services = Services::default().start();
        let addr = format!("/local/gsb-api/test/{}", uuid::Uuid::new_v4());
        let external = ExternalService.start();
        ya_service_bus::actix_rpc::try_bind_raw(&addr, external.recipient()).unwrap();

        let error = services
            .send(Bind {
                components: vec!["Call".to_string()],
                addr_prefix: addr.clone(),
                owner: "alice".to_string(),
            })
            .await
            .unwrap()
            .unwrap_err();

        assert!(matches!(error, BindError::DuplicatedService(_)));
        assert!(ya_service_bus::actix_rpc::unbind_raw(&addr));
    }
}
