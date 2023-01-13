
use actix::prelude::*;
use async_trait::async_trait;
use crate::{GsbApiError, WsMessagesHandler, WsRequest, WsResponse, WsResult};
use actix::prelude::*;
use actix::{Actor, Addr, Context, Handler, Recipient, Message};
// use actix_web::Handler;
use futures::channel::oneshot::{self, Canceled, Sender, Receiver};
use lazy_static::lazy_static;
use serde::Deserialize;
use std::pin::Pin;
use std::{
    collections::{HashMap, HashSet},
    convert::{TryFrom, TryInto},
    future::Future,
    result::Result::{Err, Ok},
    sync::{Arc, Mutex, RwLock},
};
use ya_core_model::gftp::{self, GetChunk, GetMetadata, GftpChunk, GftpMetadata};
use ya_service_bus::{
    actix_rpc as ubus, typed as bus,
    untyped::{Fn4Handler, Fn4HandlerExt, RawHandler},
    RpcMessage, RpcRawCall,
};

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

#[derive(Message)]
#[rtype(result = "Result<(), anyhow::Error>")]
pub(crate) struct ABind {
    pub components: Vec<String>,
    pub addr: String,
}

impl Handler<ABind> for AServices {
    type Result = <ABind as Message>::Result;

    fn handle(&mut self, msg: ABind, ctx: &mut Self::Context) -> Self::Result {
        let addr = msg.addr.clone();
        if self.services.contains_key(&addr) {
            anyhow::bail!("Service bound on address: {addr}");
        }
        let service = AService::from(msg).start();
        self.services.insert(addr, service);
        Ok(())
    }
}

#[derive(Message)]
#[rtype(result = "Result<(), ya_service_bus::Error>")]
pub(crate) struct AUnbind {
    pub addr: String
}

impl Handler<AUnbind> for AServices {
    type Result = <AUnbind as Message>::Result;

    fn handle(&mut self, msg: AUnbind, ctx: &mut Self::Context) -> Self::Result {
        todo!()
    }
}


#[derive(Message)]
#[rtype(result = "Result<Addr<AService>, ya_service_bus::Error>")]
pub(crate) struct AFind {
    pub addr: String,
}

impl Handler<AFind> for AServices {
    type Result = <AFind as Message>::Result;

    fn handle(&mut self, msg: AFind, ctx: &mut Self::Context) -> Self::Result {
        todo!()
    }
}

#[derive(Message)]
#[rtype(result = "Result<(), anyhow::Error>")]
pub(crate) struct AWsBind {
    pub ws: Addr<WsMessagesHandler>,
    pub addr: String,
}

impl Handler<AWsBind> for AServices {
    type Result = <AWsBind as Message>::Result;

    fn handle(&mut self, msg: AWsBind, ctx: &mut Self::Context) -> Self::Result {
        todo!()
    }
}

pub(crate) struct AService {
    addr: String,
    components: Vec<String>,
    msg_handler: Box<dyn MessagesHandler>,
}

impl From<ABind> for AService {
    fn from(value: ABind) -> Self {
        let msg_handler = BufferingHandler {};
        AService { addr: value.addr, components: value.components, msg_handler: Box::new(msg_handler) }
    }
}

impl Actor for AService {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        _ = ya_service_bus::actix_rpc::bind_raw(&self.addr, ctx.address().recipient());
    }

    fn stopped(&mut self, ctx: &mut Self::Context) {
        // unbind here
    }
}

impl Handler<RpcRawCall> for AService {
    type Result = ResponseFuture<Result<Vec<u8>, ya_service_bus::Error>>;

    fn handle(&mut self, msg: RpcRawCall, ctx: &mut Self::Context) -> Self::Result {
        let id = uuid::Uuid::new_v4().to_string();
        let addr = msg.addr;
        log::debug!("Msg addr: {addr}, id: {id}");
        //TODO how to get component name (from addr?)?
        let component = "GetMetadata".to_string();
        let msg = WsRequest {component, id, msg: msg.body };
        let msg_handling_future = self.msg_handler.handle_request(msg);
        Box::pin(async {
            //TODO define some error types
            let receiver = msg_handling_future.await
                .map_err(|err| ya_service_bus::Error::GsbFailure(err.to_string()))?;
            let raw_msg = receiver.await
                .map_err(|err| ya_service_bus::Error::GsbFailure(err.to_string()))?
                .map_err(|err| ya_service_bus::Error::GsbFailure(err.to_string()))?;
            Ok(raw_msg.msg)
        })
    }

}

impl Handler<WsResponse> for AService {
    type Result = <WsResponse as Message>::Result;

    fn handle(&mut self, msg: WsResponse, ctx: &mut Self::Context) -> Self::Result {
        // self.
        todo!()
    }
}

#[derive(Message)]
#[rtype(result = "Result<(), anyhow::Error>")]
pub(crate) struct Listen {
    pub listener: Addr<WsMessagesHandler>,
}

impl Handler<Listen> for AService {
    type Result = <Listen as Message>::Result;

    fn handle(&mut self, msg: Listen, ctx: &mut Self::Context) -> Self::Result {
        let ws_handler = msg.listener;
        self.msg_handler = Box::new(SendingHandler::new(ws_handler));
        todo!()
    }
}

trait MessagesHandler {
    fn handle_request(&mut self, msg: WsRequest) -> Pin<Box<dyn Future<Output=Result<Receiver<WsResult>, anyhow::Error>>>>;

    fn handle_response(&mut self, msg: WsResult) -> Result<(), anyhow::Error>;
}

struct BufferingHandler {

}

impl MessagesHandler for BufferingHandler {
    fn handle_request(&mut self, msg: WsRequest) ->  Pin<Box<dyn Future<Output=Result<Receiver<WsResult>, anyhow::Error>>>> {
        todo!("Should buffer pending requests")
    }

    fn handle_response(&mut self, msg: WsResult) -> Result<(), anyhow::Error> {
        todo!("Probably should fail here - SendingHandler should handle responses")
    }
}

struct SendingHandler {
    pending_senders: HashMap<String, Sender<WsResult>>,
    ws_handler: Addr<WsMessagesHandler>
}

impl SendingHandler {
    fn new(ws_handler: Addr<WsMessagesHandler>) -> Self {
        SendingHandler { pending_senders: HashMap::new(), ws_handler }
    }
}

impl MessagesHandler for SendingHandler {
    fn handle_request(&mut self, msg: WsRequest) -> Pin<Box<dyn Future<Output=Result<Receiver<WsResult>, anyhow::Error>>>> {
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

    fn handle_response(&mut self, msg: WsResult) -> Result<(), anyhow::Error> {
        let msg = msg
            // .map_err(|err| /* no idea */)
            .map_err(|err| anyhow::anyhow!(err))
            ?;
        
        match self.pending_senders.remove(&msg.id) {
            Some(sender) => {
                //TODO how to handle errors
                sender.send(Ok(msg));
            }
            None => {
                anyhow::bail!("Unable to respond to: {:?}", msg);
            }
        }
        Ok(())
    }

    
}


// pub struct ARequest {

// }

// impl Message for ARequest {
//     type Result = Result<(), ya_service_bus::Error>;
// }

// pub struct AResponse {

// }

// impl Message for AResponse {
//     type Result = Result<(), ya_service_bus::Error>;
// }

///
/// 

#[derive(Default)]
pub(crate) struct GsbServices {
    ws_requests_dst: HashMap<String, Arc<RwLock<Option<Addr<WsMessagesHandler>>>>>,
    ws_responses_dst: HashMap<String, Arc<RwLock<HashMap<String, oneshot::Sender<WsResult>>>>>,
}

// impl Handler<WsRequest> for GsbServices {
//     type Result = WsResult;

//     fn handle(&mut self, msg: WsRequest, ctx: &mut Self::Context) -> Self::Result {
//         todo!()
//     }
// }

// impl Actor for GsbServices {
//     type Context = Context<Self>;
    
// }

impl GsbServices {
    pub fn bind(&mut self, components: HashSet<&str>, path: &str) -> Result<(), GsbApiError> {
        let ws_calls = self.ws_responses_dst(path);
        let ws_request_dst = self.ws_request_dst(path);
        for component in components {
            Self::bind_raw(path, component, ws_calls.clone(), ws_request_dst.clone())?;
        }
        Ok(())
    }

    fn bind_raw(
        path: &str,
        component: &str,
        senders_map: Arc<RwLock<HashMap<String, oneshot::Sender<WsResult>>>>,
        request_dst: Arc<RwLock<Option<Addr<WsMessagesHandler>>>>,
    ) -> Result<(), GsbApiError> {
        let senders_map = senders_map.clone();
        let request_dst = request_dst.clone();
        let addr = path.to_string();
        let component = component.to_string();
        let rpc = move |addr: &str, path: &str, msg: &[u8]| {
            let component = component.to_string();
            let senders_map = senders_map.clone();
            let id = uuid::Uuid::new_v4().to_string();
            let request_dst = request_dst.clone();
            let msg = msg.to_vec();
            async move {
                // let ws_request = (id.clone(), msg).try_into()?;
                let ws_request = WsRequest {
                    id: id.clone(),
                    component,
                    msg,
                };
                let (ws_sender, ws_receiver) = oneshot::channel();
                {
                    let mut senders = senders_map.write().unwrap();
                    senders.insert(id, ws_sender);
                }
                //TODO handle it properly
                let request_dst = request_dst.read().unwrap();
                match &*request_dst {
                    Some(request_dst) => {
                        log::info!("Sending msg");
                        if let Err(err) = request_dst.send(ws_request).await {
                            log::info!("Mailbox closed: {}", err);
                        }
                    }
                    None => {
                        //TODO handle it
                        todo!("handle not initialized/uninitialised request addr");
                    }
                };
                let response = ws_receiver
                    .await
                    .map(|resp| resp)
                    .map_err(|err| ya_service_bus::Error::GsbFailure(err.to_string()))?
                    .map_err(|err| ya_service_bus::Error::GsbFailure(err.to_string()))?;
                Ok(response.msg)
            }
        };
        log::info!("Binding service: {addr}");
        let _ = ya_service_bus::untyped::subscribe(&addr, rpc, ());
        // let _ = ya_service_bus::typed::bind_with_caller(addr, f)
        // ya_service_bus::actix_rpc::bind_raw(addr, actor)
        Ok(())
    }

    pub fn ws_request_dst(&mut self, path: &str) -> Arc<RwLock<Option<Addr<WsMessagesHandler>>>> {
        match self.ws_requests_dst.get_mut(path) {
            Some(request_dst) => request_dst.clone(),
            None => {
                let request_dst = Arc::new(RwLock::new(None));
                self.ws_requests_dst
                    .insert(path.to_string(), request_dst.clone());
                request_dst
            }
        }
    }

    pub fn ws_responses_dst(
        &mut self,
        path: &str,
    ) -> Arc<RwLock<HashMap<String, oneshot::Sender<WsResult>>>> {
        match self.ws_responses_dst.get_mut(path) {
            Some(calls_map) => calls_map.clone(),
            None => {
                let calls_map = Arc::new(RwLock::new(HashMap::new()));
                self.ws_responses_dst
                    .insert(path.to_string(), calls_map.clone());
                calls_map
            }
        }
    }
}

trait ServiceBinder<MSG>
where
    MSG: RpcMessage,
    (String, MSG): TryInto<WsRequest, Error = <MSG as RpcMessage>::Error>,
    MSG::Item: TryFrom<WsResult, Error = <MSG as RpcMessage>::Error>,
{
    fn bind_service(
        path: &str,
        senders_map: Arc<RwLock<HashMap<String, oneshot::Sender<WsResult>>>>,
        request_dst: Arc<RwLock<Option<Addr<WsMessagesHandler>>>>,
    ) -> Result<(), GsbApiError> {
        let _ = bus::bind_with_caller(&path, move |path, packet: MSG| {
            let senders_map = senders_map.clone();
            let id = uuid::Uuid::new_v4().to_string();
            let request_dst = request_dst.clone();
            async move {
                let ws_request = (id.clone(), packet).try_into()?;
                let (ws_sender, ws_receiver) = oneshot::channel();
                {
                    let mut senders = senders_map.write().unwrap();
                    senders.insert(id, ws_sender);
                }
                //TODO handle it properly
                let request_dst = request_dst.read().unwrap();
                match &*request_dst {
                    Some(request_dst) => {
                        log::info!("Sending {:?}", ws_request);
                        if let Err(err) = request_dst.send(ws_request).await {
                            log::info!("Mailbox closed: {}", err);
                        }
                    }
                    None => {
                        //TODO handle it
                        todo!("handle not initialized/uninitialised request addr");
                    }
                };
                let ws_res = ws_receiver.await.map_err(Self::map_canceled)?;
                let response = MSG::Item::try_from(ws_res)?;
                Ok(response)
            }
        });

        Ok(())
    }

    fn map_canceled(err: Canceled) -> MSG::Error;
}
