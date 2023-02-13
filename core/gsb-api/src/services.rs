use crate::service::{DropMessages, Service};
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
    services: HashMap<String, Addr<Service>>,
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
}

#[derive(Message, Debug)]
#[rtype(result = "Result<(), BindError>")]
pub(crate) struct Bind {
    pub components: Vec<String>,
    pub addr_prefix: String,
}

impl Handler<Bind> for Services {
    type Result = <Bind as Message>::Result;

    fn handle(&mut self, msg: Bind, _ctx: &mut Self::Context) -> Self::Result {
        if msg.addr_prefix.is_empty() {
            return Err(BindError::InvalidService(
                "Cannod bind service. Empty prefix.".to_string(),
            ));
        }
        let addr = msg.addr_prefix.clone();
        if self.services.contains_key(&addr) {
            return Err(BindError::DuplicatedService(addr));
        }
        let service = Service::from(msg).start();
        log::debug!("Created service: {:?}", service);
        self.services.insert(addr, service);
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
        let some_service = self.services.remove(&msg.addr);
        Box::pin(async move {
            match some_service {
                Some(service) => {
                    log::debug!("Unbinding service: {}", msg.addr);
                    let error = CloseReason {
                        code: ws::CloseCode::Normal,
                        description: Some(format!("Unbinding service: {}", msg.addr)),
                    };
                    Ok(service.send(DropMessages { reason: error }).await?)
                }
                None => Err(UnbindError::ServiceNotFound(format!(
                    "Cannot find service: {}",
                    msg.addr
                ))),
            }
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
}

impl Handler<Find> for Services {
    type Result = <Find as Message>::Result;

    fn handle(&mut self, msg: Find, _ctx: &mut Self::Context) -> Self::Result {
        if msg.addr.is_empty() {
            return Err(FindError::EmptyAddress);
        }
        if let Some(service) = self.services.get(&msg.addr) {
            return Ok(service.clone());
        }
        Err(FindError::ServiceNotFound(msg.addr))
    }
}
