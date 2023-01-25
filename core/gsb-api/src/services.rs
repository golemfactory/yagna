use crate::{GsbApiError, WsMessagesHandler, WsRequest, WsResponse};
use actix::prelude::*;
use actix::{Actor, Addr, Context, Handler, Message};
use anyhow::anyhow;
use futures::channel::oneshot::{self, Receiver, Sender};
use lazy_static::lazy_static;
use std::pin::Pin;
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    result::Result::{Err, Ok},
};
use ya_service_bus::{RpcMessage, RpcRawCall};

lazy_static! {
    pub(crate) static ref SERVICES: Addr<AServices> = AServices::default().start();
}

trait GsbCaller {
    fn call<'MSG, REQ: RpcMessage + Into<WsRequest>, RES: RpcMessage + From<WsResponse>>(
        self,
        path: String,
        req: REQ,
    ) -> dyn Future<Output = Result<RES, RES::Error>>;
}

///
#[derive(Default)]
pub(crate) struct AServices {
    services: HashMap<String, Addr<AService>>,
}

impl Actor for AServices {
    type Context = Context<Self>;
}

#[derive(Message, Debug)]
#[rtype(result = "Result<(), anyhow::Error>")]
pub(crate) struct ABind {
    pub components: Vec<String>,
    pub addr_prefix: String,
}

impl Handler<ABind> for AServices {
    type Result = <ABind as Message>::Result;

    fn handle(&mut self, msg: ABind, _ctx: &mut Self::Context) -> Self::Result {
        let addr = msg.addr_prefix.clone();
        if self.services.contains_key(&addr) {
            anyhow::bail!("Service bound on address: {addr}");
        }
        let service = AService::from(msg).start();
        self.services.insert(addr, service);
        Ok(())
    }
}

#[derive(Message, Debug)]
#[rtype(result = "Result<(), ya_service_bus::Error>")]
pub(crate) struct AUnbind {
    pub addr: String,
}

impl Handler<AUnbind> for AServices {
    type Result = <AUnbind as Message>::Result;

    fn handle(&mut self, _msg: AUnbind, _ctx: &mut Self::Context) -> Self::Result {
        todo!()
    }
}

#[derive(Message, Debug)]
#[rtype(result = "Result<Addr<AService>, anyhow::Error>")]
pub(crate) struct AFind {
    pub addr: String,
}

impl Handler<AFind> for AServices {
    type Result = <AFind as Message>::Result;

    fn handle(&mut self, msg: AFind, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(service) = self.services.get(&msg.addr) {
            return Ok(service.clone());
        }
        anyhow::bail!("No service for: {:?}", msg)
    }
}

#[derive(Message, Debug)]
#[rtype(result = "Result<(), anyhow::Error>")]
pub(crate) struct AWsBind {
    pub ws: Addr<WsMessagesHandler>,
    pub addr: String,
}

impl Handler<AWsBind> for AServices {
    type Result = <AWsBind as Message>::Result;

    fn handle(&mut self, _msg: AWsBind, _ctx: &mut Self::Context) -> Self::Result {
        todo!()
    }
}

pub(crate) struct AService {
    addr_prefix: String,
    addresses: HashSet<String>,
    msg_handler: Box<dyn MessagesHandler>,
}

impl AService {
    fn addr_prefix_to_component(addr: &str) -> String {
        addr.chars()
            .rev()
            .take_while(|ch| ch != &'/')
            .collect::<Vec<char>>()
            .iter()
            .rev()
            .collect()
    }
}

impl From<ABind> for AService {
    fn from(bind: ABind) -> Self {
        let msg_handler = Box::new(BufferingHandler {});
        // convert to error and return it when e.g. components empty
        let addr_prefix = bind.addr_prefix;
        let mut addresses = HashSet::new();
        for component in bind.components {
            addresses.insert(format!("{addr_prefix}/{component}"));
        }
        AService {
            addr_prefix,
            addresses,
            msg_handler: msg_handler,
        }
    }
}

impl Actor for AService {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        _ = ya_service_bus::actix_rpc::bind_raw(&self.addr_prefix, ctx.address().recipient());
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        // unbind here
    }
}

impl Handler<RpcRawCall> for AService {
    type Result = ResponseFuture<Result<Vec<u8>, ya_service_bus::Error>>;

    fn handle(&mut self, msg: RpcRawCall, _ctx: &mut Self::Context) -> Self::Result {
        let addr = msg.addr;
        log::info!("Incoming GSB RAW call (addr: {addr})");

        if !self.addresses.contains(&addr) {
            //TODO use futures::ready! or sth
            return Box::pin(async move {
                Err(ya_service_bus::Error::GsbBadRequest(format!(
                    "No supported msg type for addr: {}",
                    addr
                )))
            });
        }
        let id = uuid::Uuid::new_v4().to_string();
        log::debug!("Msg addr: {addr}, id: {id}");
        let component = AService::addr_prefix_to_component(&addr);
        let msg = WsRequest {
            component,
            id,
            payload: msg.body,
        };
        let msg_handling_future = self.msg_handler.handle_request(msg);
        Box::pin(async {
            let receiver = match msg_handling_future.await {
                Ok(receiver) => receiver,
                Err(err) => {
                    log::error!("Sending error (runtime) GSB response: {err}");
                    return Err(ya_service_bus::Error::GsbFailure(err.to_string()));
                }
            };
            let ws_response = match receiver.await {
                Ok(ws_response) => ws_response,
                Err(err) => {
                    log::error!("Sending error (internal) GSB response: {err}");
                    return Err(ya_service_bus::Error::GsbFailure(err.to_string()));
                }
            };
            log::info!("Sending GSB response: {ws_response:?}");
            match ws_response.response {
                crate::WsResponseMsg::Message(gsb_msg) => Ok(gsb_msg),
                crate::WsResponseMsg::Error(err) => {
                    log::error!("Sending error GSB response: {err}");
                    match err {
                        GsbApiError::BadRequest => {
                            Err(ya_service_bus::Error::GsbBadRequest(err.to_string()))
                        }
                        GsbApiError::InternalError(_) => {
                            Err(ya_service_bus::Error::GsbFailure(err.to_string()))
                        }
                        GsbApiError::Any(_) => {
                            Err(ya_service_bus::Error::GsbFailure(err.to_string()))
                        }
                    }
                }
            }
        })
    }
}

impl Handler<WsResponse> for AService {
    type Result = <WsResponse as Message>::Result;

    fn handle(&mut self, msg: WsResponse, _ctx: &mut Self::Context) -> Self::Result {
        self.msg_handler
            .handle_response(msg)
            .map_err(|err| anyhow!(format!("Failed to handle response: {:?}", err)))
    }
}

#[derive(Message, Debug)]
#[rtype(result = "Result<(), anyhow::Error>")]
pub(crate) struct Listen {
    pub listener: Addr<WsMessagesHandler>,
}

impl Handler<Listen> for AService {
    type Result = <Listen as Message>::Result;

    fn handle(&mut self, msg: Listen, _ctx: &mut Self::Context) -> Self::Result {
        let ws_handler = msg.listener;
        self.msg_handler = Box::new(SendingHandler::new(ws_handler));
        //TODO should fail if it already has SendingHandler
        Ok(())
    }
}

trait MessagesHandler {
    fn handle_request(
        &mut self,
        msg: WsRequest,
    ) -> Pin<Box<dyn Future<Output = Result<Receiver<WsResponse>, anyhow::Error>>>>;

    fn handle_response(&mut self, msg: WsResponse) -> Result<(), WsResponse>;
}

struct BufferingHandler {}

impl MessagesHandler for BufferingHandler {
    fn handle_request(
        &mut self,
        _msg: WsRequest,
    ) -> Pin<Box<dyn Future<Output = Result<Receiver<WsResponse>, anyhow::Error>>>> {
        todo!("Should buffer pending requests")
    }

    fn handle_response(&mut self, _msg: WsResponse) -> Result<(), WsResponse> {
        todo!("Probably should fail here - SendingHandler should handle responses")
    }
}

struct SendingHandler {
    pending_senders: HashMap<String, Sender<WsResponse>>,
    ws_handler: Addr<WsMessagesHandler>,
}

impl SendingHandler {
    fn new(ws_handler: Addr<WsMessagesHandler>) -> Self {
        SendingHandler {
            pending_senders: HashMap::new(),
            ws_handler,
        }
    }
}

impl MessagesHandler for SendingHandler {
    fn handle_request(
        &mut self,
        msg: WsRequest,
    ) -> Pin<Box<dyn Future<Output = Result<Receiver<WsResponse>, anyhow::Error>>>> {
        let id = msg.id.clone();
        let ws_handler = self.ws_handler.clone();
        let (sender, receiver) = oneshot::channel();
        self.pending_senders.insert(id, sender);
        Box::pin(async move {
            //TODO either remove handler under current `id` here, or map it as an error with `id`.
            let _ = ws_handler.send(msg).await?;
            Ok(receiver)
        })
    }

    fn handle_response(&mut self, res: WsResponse) -> Result<(), WsResponse> {
        match self.pending_senders.remove(&res.id) {
            Some(sender) => {
                log::info!("Sending response: {res:?}");
                sender.send(res)
            }
            None => Err(WsResponse {
                id: res.id.clone(),
                response: crate::WsResponseMsg::Error(GsbApiError::InternalError(format!(
                    "Unable to respond to: {:?}",
                    res.id
                ))),
            }),
        }
    }
}
