
use actix::prelude::*;
use crate::{GsbApiError, WsMessagesHandler, WsRequest, WsResponse, WsResult};
use actix::prelude::*;
use actix::{Actor, Addr, Context, Handler, Recipient, Message};
// use actix_web::Handler;
use futures::channel::oneshot::{self, Canceled};
use lazy_static::lazy_static;
use serde::Deserialize;
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

lazy_static! {
    pub(crate) static ref SERVICES: Arc<Mutex<GsbServices>> =
        Arc::new(Mutex::new(GsbServices::default()));
}

trait GsbCaller {
    fn call<'MSG, REQ: RpcMessage + Into<WsRequest>, RES: RpcMessage + From<WsResponse>>(
        self,
        path: String,
        req: REQ,
    ) -> dyn Future<Output = Result<RES, RES::Error>>;
}

///

pub(crate) struct AServices {

}

impl Actor for AServices {
    type Context = Context<Self>;
}

#[derive(Message)]
#[rtype(result = "Result<(), ya_service_bus::Error>")]
pub(crate) struct ABind {
    pub components: Vec<String>,
    pub addr: String,
}

impl Handler<ABind> for AServices {
    type Result = <ABind as Message>::Result;

    fn handle(&mut self, msg: ABind, ctx: &mut Self::Context) -> Self::Result {
        todo!()
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
}

impl Actor for AService {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // bind here
    }

    fn stopped(&mut self, ctx: &mut Self::Context) {
        // unbind here
    }
}

impl Handler<RpcRawCall> for AService {
    type Result = Result<Vec<u8>, ya_service_bus::Error>;

    fn handle(&mut self, msg: RpcRawCall, ctx: &mut Self::Context) -> Self::Result {
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
        todo!()
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
