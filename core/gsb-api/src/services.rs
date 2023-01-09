use crate::{GsbApiError, WsMessagesHandler, WsRequest, WsResponse, WsResult};
use actix::{Actor, Addr, Context, Handler, Recipient};
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

#[derive(Default)]
pub(crate) struct GsbServices {
    ws_requests_dst: HashMap<String, Arc<RwLock<Option<Addr<WsMessagesHandler>>>>>,
    ws_responses_dst: HashMap<String, Arc<RwLock<HashMap<String, oneshot::Sender<WsResult>>>>>,
}

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
        let addr = format!("{path}/{component}");
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
