use crate::services::Bind;
use crate::{WsDisconnect, WsMessagesHandler, WsRequest, WsResponse};
use actix::prelude::*;
use actix::{Actor, Addr, Context, Handler, Message};
use actix_http::ws::CloseReason;
use anyhow::anyhow;
use futures::channel::oneshot::{self, Receiver, Sender};
use std::pin::Pin;
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    result::Result::{Err, Ok},
};
use thiserror::Error;
use ya_service_bus::RpcRawCall;

pub(crate) struct Service {
    addr_prefix: String,
    addresses: HashSet<String>,
    msg_handler: Box<dyn MessagesHandler>,
}

impl Service {
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

impl From<Bind> for Service {
    fn from(bind: Bind) -> Self {
        let msg_handler = Box::new(BufferingHandler {});
        // convert to error and return it when e.g. components empty
        let addr_prefix = bind.addr_prefix;
        let mut addresses = HashSet::new();
        for component in bind.components {
            addresses.insert(format!("{addr_prefix}/{component}"));
        }
        Service {
            addr_prefix,
            addresses,
            msg_handler,
        }
    }
}

#[derive(Error, Debug)]
pub(crate) enum DisconnectError {
    #[error("Failed to disconnect GSB services: {0}")]
    FailedGSB(String),
    #[error("Failed to disconnect WS services: {0}")]
    FailedWS(String),
}

#[derive(Message, Debug)]
#[rtype(result = "Result<(), DisconnectError>")]
pub(crate) struct Disconnect {
    pub(crate) msg: String,
}

impl Actor for Service {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        _ = ya_service_bus::actix_rpc::bind_raw(&self.addr_prefix, ctx.address().recipient());
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        // unbind here
    }
}

impl Handler<Disconnect> for Service {
    type Result = ResponseFuture<<Disconnect as Message>::Result>;

    fn handle(&mut self, msg: Disconnect, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
        let disconnect_future = self.msg_handler.disconnect(msg);
        Box::pin(async { disconnect_future.await })
    }
}

impl Handler<RpcRawCall> for Service {
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
        let component = Service::addr_prefix_to_component(&addr);
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
                crate::WsResponseMsg::Error(err) => Err(err),
            }
        })
    }
}

impl Handler<WsResponse> for Service {
    type Result = <WsResponse as Message>::Result;

    fn handle(&mut self, msg: WsResponse, _ctx: &mut Self::Context) -> Self::Result {
        self.msg_handler
            .handle_response(msg)
            .map_err(|err| anyhow!(format!("Failed to handle response: {:?}", err)))
    }
}

#[derive(Message, Debug)]
#[rtype(result = "()")]
pub(crate) struct Listen {
    pub listener: Addr<WsMessagesHandler>,
}

impl Handler<Listen> for Service {
    type Result = <Listen as Message>::Result;

    fn handle(&mut self, msg: Listen, _ctx: &mut Self::Context) -> Self::Result {
        let ws_handler = msg.listener;
        self.msg_handler = Box::new(SendingHandler::new(ws_handler));
        //TODO should fail if it already has SendingHandler
        // Ok(())
    }
}

trait MessagesHandler {
    fn handle_request(
        &mut self,
        msg: WsRequest,
    ) -> Pin<Box<dyn Future<Output = Result<Receiver<WsResponse>, anyhow::Error>>>>;

    fn handle_response(&mut self, msg: WsResponse) -> Result<(), WsResponse>;

    fn disconnect<'a>(
        &mut self,
        msg: Disconnect,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DisconnectError>> + Send + 'a>>;
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

    fn disconnect<'a>(
        &mut self,
        _msg: Disconnect,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DisconnectError>> + Send + 'a>> {
        todo!("Send error to all receivers")
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
                response: crate::WsResponseMsg::Error(ya_service_bus::Error::GsbFailure(format!(
                    "Unable to respond to: {:?}",
                    res.id
                ))),
            }),
        }
    }

    fn disconnect<'a>(
        &mut self,
        msg: Disconnect,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DisconnectError>> + Send + 'a>> {
        for (addr, sender) in self.pending_senders.drain() {
            log::debug!("Closing GSB connection: {}", addr);
            let _ = sender.send(WsResponse {
                id: addr,
                response: crate::WsResponseMsg::Error(ya_service_bus::Error::Closed(
                    msg.msg.clone(),
                )),
            });
        }
        let disconnect_fut = self.ws_handler.send(WsDisconnect(CloseReason {
            code: actix_http::ws::CloseCode::Normal,
            description: Some(msg.msg.clone()),
        }));
        Box::pin(async move {
            disconnect_fut
                .await
                .map_err(|err| DisconnectError::FailedWS(err.to_string()))
        })
    }
}
