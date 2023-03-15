use crate::services::Bind;
use crate::{GsbError, WsDisconnect, WsMessagesHandler, WsRequest, WsResponse, WsResponseMsg};
use actix::prelude::*;
use actix::{Actor, Addr, Context, Handler, Message};
use actix_http::ws::CloseReason;
use anyhow::anyhow;
use futures::channel::oneshot::{self, Receiver, Sender};
use futures::future::LocalBoxFuture;
use futures::{FutureExt, ready};
use std::pin::Pin;
use std::{
    collections::{HashMap, HashSet},
    future,
    future::Future,
    mem,
    result::Result::{Err, Ok},
};
use std::task::Poll;
use std::time::Duration;
use tokio::time::Sleep;
use ya_service_bus::RpcRawCall;

pub(crate) struct Service {
    /// Service prefix
    addr_prefix: String,
    /// Service addresses with same prefix but different RpcMessage types.
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

#[cfg(test)]
#[test]
fn test_addr_prefix_to_component() {
    assert_eq!(
        Service::addr_prefix_to_component("/public/gftp/id_of_shared_data"),
        "id_of_shared_data"
    );
}

impl From<Bind> for Service {
    fn from(bind: Bind) -> Self {
        let msg_handler: Box<dyn MessagesHandler> = Box::new(BufferingHandler::default());
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

impl Actor for Service {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        _ = ya_service_bus::actix_rpc::bind_raw(&self.addr_prefix, ctx.address().recipient());
    }
}

#[derive(Message, Debug)]
#[rtype(result = "()")]
pub(crate) struct DropMessages {
    pub(crate) reason: CloseReason,
}

impl Handler<DropMessages> for Service {
    type Result = ResponseFuture<<DropMessages as Message>::Result>;

    fn handle(&mut self, msg: DropMessages, _: &mut Self::Context) -> Self::Result {
        let disconnect_future = self.msg_handler.disconnect(msg);
        Box::pin(async { disconnect_future.await })
    }
}

impl Handler<RpcRawCall> for Service {
    type Result = ResponseFuture<Result<Vec<u8>, GsbError>>;

    fn handle(&mut self, msg: RpcRawCall, _ctx: &mut Self::Context) -> Self::Result {
        let addr = msg.addr;
        log::debug!("Incoming GSB RAW call (addr: {addr})");

        if !self.addresses.contains(&addr) {
            //TODO use futures::ready! or sth
            return Box::pin(async move {
                Err(GsbError::GsbBadRequest(format!(
                    "No supported msg type for addr: {addr}"
                )))
            });
        }
        let id = uuid::Uuid::new_v4().to_string();
        log::debug!("GSB RAW call msg id: {id}");
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
                    return Err(GsbError::GsbFailure(err.to_string()));
                }
            };
            let ws_response = match receiver.await {
                Ok(ws_response) => ws_response,
                Err(err) => {
                    log::error!("Sending error (internal) GSB response: {err}");
                    return Err(GsbError::GsbFailure(err.to_string()));
                }
            };
            log::debug!("Sending GSB RAW call response: (id: {})", ws_response.id);
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
            .map_err(|err| anyhow!(format!("Failed to handle response. Err: {err:?}")))
    }
}

/// Message making message handler to relay messages.
#[derive(Message, Debug)]
#[rtype(result = "()")]
pub(crate) struct StartRelaying {
    pub ws_handler: Addr<WsMessagesHandler>,
}

impl Handler<StartRelaying> for Service {
    type Result = ResponseFuture<<StartRelaying as Message>::Result>;

    fn handle(&mut self, msg: StartRelaying, ctx: &mut Self::Context) -> Self::Result {
        if let Some((next_handler, sync_fut)) = self.msg_handler.start_relaying(msg.ws_handler, ctx)
        {
            self.msg_handler = next_handler;
            sync_fut
        } else {
            future::ready(()).boxed_local()
        }
    }
}

/// Message making message handler to buffer messages and return old WS messages handler (if there was any).
#[derive(Message, Debug)]
#[rtype(result = "Option<Addr<WsMessagesHandler>>")]
pub(crate) struct StartBuffering;

impl Handler<StartBuffering> for Service {
    type Result = <StartBuffering as Message>::Result;

    fn handle(&mut self, _: StartBuffering, ctx: &mut Self::Context) -> Self::Result {
        log::debug!("Start buffering.");
        let old_ws_handler = self.msg_handler.ws_handler();
        if let Some(next_handler) = self.msg_handler.start_buffering() {
            self.msg_handler = next_handler;
        }
        old_ws_handler
    }
}

trait MessagesHandler {
    fn start_buffering(&mut self) -> Option<Box<dyn MessagesHandler>>;

    // Returns new handler and sync future.
    fn start_relaying(
        &mut self,
        ws_handler: Addr<WsMessagesHandler>,
        ctx: &mut <Service as Actor>::Context,
    ) -> Option<(Box<dyn MessagesHandler>, LocalBoxFuture<'static, ()>)>;

    fn handle_request(
        &mut self,
        msg: WsRequest,
    ) -> Pin<Box<dyn Future<Output = Result<Receiver<WsResponse>, anyhow::Error>>>>;

    fn handle_response(&mut self, msg: WsResponse) -> Result<(), WsResponse>;

    fn disconnect(
        &mut self,
        msg: DropMessages,
    ) -> LocalBoxFuture<()>;

    fn ws_handler(&self) -> Option<Addr<WsMessagesHandler>>;
}

fn drop_messages(
    pending_senders: &mut HashMap<String, Sender<WsResponse>>,
    drop_messages: &DropMessages,
) {
    for (addr, sender) in pending_senders.drain() {
        log::debug!("Closing GSB connection: {}", addr);
        let _ = sender.send(WsResponse {
            id: addr,
            response: WsResponseMsg::from(drop_messages),
        });
    }
}

async fn send_pending_requests(
    mut pending_msgs: Vec<WsRequest>,
    ws_handler: Addr<WsMessagesHandler>,
    service: Addr<Service>,
) {
    while let Some(msg) = pending_msgs.pop() {
        log::debug!("Sending buffered message: {}", msg.id);
        let id = msg.id.clone();
        if let Some(error) = match ws_handler.send(msg).await {
            Ok(Err(error)) => Some(GsbError::GsbFailure(format!(
                "Failed to forward buffered request: {error}"
            ))),
            Err(error) => Some(GsbError::GsbFailure(format!(
                "Failed to forward buffered request. Internal error: {error}"
            ))),
            _ => None,
        } {
            let response = WsResponse {
                id: id.clone(),
                response: WsResponseMsg::Error(error),
            };
            if let Err(error) = service.send(response).await {
                log::error!("Failed to send GSB error msg for id: {id}. Err: {error}");
            }
        }
    }
}

/// Messages handler buffering GSB requests until WS (re)connects.
#[derive(Debug, Default)]
struct BufferingHandler {
    pending_senders: HashMap<String, Sender<WsResponse>>,
    pending_msgs: Vec<WsRequest>,
}

impl MessagesHandler for BufferingHandler {
    fn start_buffering(&mut self) -> Option<Box<dyn MessagesHandler>> {
        None
    }

    fn start_relaying(
        &mut self,
        ws_handler: Addr<WsMessagesHandler>,
        ctx: &mut <Service as Actor>::Context,
    ) -> Option<(Box<dyn MessagesHandler>, LocalBoxFuture<'static, ()>)> {
        let pending_senders = mem::take(&mut self.pending_senders);
        let pending_msgs = mem::take(&mut self.pending_msgs);

        let sync_future = {
            let service = ctx.address();
            let ws_handler = ws_handler.clone();
            send_pending_requests(pending_msgs, ws_handler, service)
        }
        .boxed_local();

        Some((
            Box::new(RelayingHandler {
                pending_senders,
                ws_handler,
            }),
            sync_future,
        ))
    }

    fn handle_request(
        &mut self,
        msg: WsRequest,
    ) -> Pin<Box<dyn Future<Output = Result<Receiver<WsResponse>, anyhow::Error>>>> {
        log::debug!("Buffering handler request (id: {})", msg.id);
        let id = msg.id.clone();
        let (sender, receiver) = oneshot::channel();
        self.pending_senders.insert(id, sender);
        self.pending_msgs.push(msg);
        Box::pin(actix::fut::ready(Ok(receiver)))
    }

    fn handle_response(&mut self, msg: WsResponse) -> Result<(), WsResponse> {
        log::error!(
            "Buffering handler response. WsResponse should never be send to BufferingHandler"
        );
        let id = msg.id;
        let response_error = GsbError::GsbFailure("Unexpected response".to_string());
        let response = crate::WsResponseMsg::Error(response_error);
        Err(WsResponse { id, response })
    }

    fn disconnect(
        &mut self,
        drop_messages_msg: DropMessages,
    ) -> LocalBoxFuture<()> {
        drop_messages(&mut self.pending_senders, &drop_messages_msg);
        log::debug!("Disconnecting buffering WS response handler");
        Box::pin(actix::fut::ready(()))
    }

    fn ws_handler(&self) -> Option<Addr<WsMessagesHandler>> {
        None
    }
}

#[derive(Debug)]
/// Messages handler relaying GSB requests to WS and sending responses back to GSB.
struct RelayingHandler {
    pending_senders: HashMap<String, Sender<WsResponse>>,
    ws_handler: Addr<WsMessagesHandler>,
}

impl MessagesHandler for RelayingHandler {
    fn start_buffering(&mut self) -> Option<Box<dyn MessagesHandler>> {
        let pending_senders = mem::take(&mut self.pending_senders);
        let pending_msgs = Default::default();
        Some(Box::new(BufferingHandler {
            pending_senders,
            pending_msgs,
        }))
    }

    fn start_relaying(
        &mut self,
        ws_handler: Addr<WsMessagesHandler>,
        _ctx: &mut <Service as Actor>::Context,
    ) -> Option<(Box<dyn MessagesHandler>, LocalBoxFuture<'static, ()>)> {
        self.ws_handler = ws_handler;
        None
    }

    fn handle_request(
        &mut self,
        msg: WsRequest,
    ) -> Pin<Box<dyn Future<Output = Result<Receiver<WsResponse>, anyhow::Error>>>> {
        log::debug!("Relaying handler request (id: {})", msg.id);
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
        log::debug!("Relaying handler response (id: {})", res.id);
        match self.pending_senders.remove(&res.id) {
            Some(sender) => {
                log::debug!("Sending response (id: {})", res.id);
                sender.send(res)
            }
            None => Err(WsResponse {
                id: res.id.clone(),
                response: crate::WsResponseMsg::Error(GsbError::GsbFailure(format!(
                    "Unable to respond to (id: {})",
                    res.id
                ))),
            }),
        }
    }

    fn disconnect(
        &mut self,
        drop_messages_msg: DropMessages,
    ) -> LocalBoxFuture<()> {
        drop_messages(&mut self.pending_senders, &drop_messages_msg);
        log::debug!("Disconnecting WS response handler");
        let disconnect_fut = self.ws_handler.send(WsDisconnect(drop_messages_msg.reason));
        Box::pin(async move {
            if let Err(err) = disconnect_fut.await {
                log::warn!("Failed to disconnect from WS. Err: {}.", err);
            };
        })
    }

    fn ws_handler(&self) -> Option<Addr<WsMessagesHandler>> {
        Some(self.ws_handler.clone())
    }
}
