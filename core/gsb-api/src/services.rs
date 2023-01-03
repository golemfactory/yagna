use crate::{GsbApiError, WsMessagesHandler, WsRequest, WsResponse, WsResult};
use actix::Addr;
use futures::channel::oneshot::{self, Canceled};
use lazy_static::lazy_static;
use std::{
    collections::{HashMap, HashSet},
    convert::{TryFrom, TryInto},
    future::Future,
    result::Result::{Err, Ok},
    sync::{Arc, Mutex, RwLock},
};
use ya_core_model::gftp::{self, GetChunk, GetMetadata, GftpChunk, GftpMetadata};
use ya_service_bus::{typed as bus, RpcMessage};

lazy_static! {
    pub(crate) static ref SERVICES: Arc<Mutex<GsbServices>> =
        Arc::new(Mutex::new(GsbServices::default()));
}

trait GsbCaller {
    fn call<REQ: RpcMessage + Into<WsRequest>, RES: RpcMessage + From<WsResponse>>(
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
            //TODO handle errors
            match component {
                "GetMetadata" => {
                    log::info!("GetMetadata {path}");
                    <Self as ServiceBinder<GetMetadata>>::bind_service(
                        path,
                        ws_calls.clone(),
                        ws_request_dst.clone(),
                    )
                    .unwrap();
                }
                "GetChunk" => {
                    log::info!("GetChunk {path}");
                    <Self as ServiceBinder<GetChunk>>::bind_service(
                        path,
                        ws_calls.clone(),
                        ws_request_dst.clone(),
                    )
                    .unwrap();
                }
                _ => return Err(GsbApiError::BadRequest),
            }
        }
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
            let path = path.clone();
            let id = uuid::Uuid::new_v4().to_string();
            let request_dst = request_dst.clone();
            async move {
                let ws_request = (path, packet).try_into()?;
                let (ws_sender, ws_receiver) = oneshot::channel();
                {
                    let mut senders = senders_map.write().unwrap();
                    senders.insert(id, ws_sender);
                }
                //TODO handle it properly
                let request_dst = request_dst.read().unwrap();
                match &*request_dst {
                    Some(request_dst) => {
                        if let Err(err) = request_dst.send(ws_request).await {
                            log::error!("Failed to handle msg: {}", err);
                        }
                    }
                    None => {
                        //TODO handle it
                        todo!("handle not initialized/uninitialised request addr");
                    }
                };
                let ws_res = ws_receiver.await.map_err(Self::map_canceled)?;
                MSG::Item::try_from(ws_res)
            }
        });

        Ok(())
    }

    fn map_canceled(err: Canceled) -> MSG::Error;
}

impl ServiceBinder<GetMetadata> for GsbServices {
    fn map_canceled(err: Canceled) -> <GetMetadata as RpcMessage>::Error {
        gftp::Error::InternalError(format!("WS request failed: {}", err))
    }
}

impl TryInto<WsRequest> for (String, GetMetadata) {
    type Error = <GetMetadata as RpcMessage>::Error;

    fn try_into(self) -> Result<WsRequest, Self::Error> {
        // let payload = flexbuffers::to_vec(self.1)
        let payload = serde_json::to_vec(&self.1)
            .map_err(|err| ya_core_model::gftp::Error::InternalError(err.to_string()))?;
        let id = self.0;
        let component = GetMetadata::ID.to_string();
        Ok(WsRequest {
            id,
            component,
            payload,
        })
    }
}

impl TryFrom<WsResult> for GftpMetadata {
    type Error = <GetMetadata as RpcMessage>::Error;

    fn try_from(res: WsResult) -> Result<Self, Self::Error> {
        match res.0 {
            Ok(ws_response) => {
                //  let response = flexbuffers::from_slice(&ws_response.payload)
                let response = serde_json::from_slice(&ws_response.payload)
                    .map_err(|err| ya_core_model::gftp::Error::InternalError(err.to_string()))?;
                Ok(response)
            }
            Err(err) => Err(ya_core_model::gftp::Error::InternalError(err.to_string())),
        }
    }
}

impl ServiceBinder<GetChunk> for GsbServices {
    fn map_canceled(err: Canceled) -> <GetChunk as RpcMessage>::Error {
        gftp::Error::InternalError(format!("WS request failed: {}", err))
    }
}

impl TryInto<WsRequest> for (String, GetChunk) {
    type Error = <GetChunk as RpcMessage>::Error;

    fn try_into(self) -> Result<WsRequest, Self::Error> {
        // let payload = flexbuffers::to_vec(self.1)
        let payload = serde_json::to_vec(&self)
            .map_err(|err| ya_core_model::gftp::Error::InternalError(err.to_string()))?;
        let component = self.0;
        let id = GetMetadata::ID.to_string();
        Ok(WsRequest {
            id,
            component,
            payload,
        })
    }
}

impl TryFrom<WsResult> for GftpChunk {
    type Error = <GetChunk as RpcMessage>::Error;

    fn try_from(res: WsResult) -> Result<Self, Self::Error> {
        match res.0 {
            Ok(ws_response) => {
                // let response = flexbuffers::from_slice(&ws_response.payload)
                let response = serde_json::from_slice(&ws_response.payload)
                    .map_err(|err| ya_core_model::gftp::Error::InternalError(err.to_string()))?;
                Ok(response)
            }
            Err(err) => Err(ya_core_model::gftp::Error::InternalError(err.to_string())),
        }
    }
}
