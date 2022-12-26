//! Provider side operations
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
    pub ws_requests_dst: HashMap<String, Arc<Addr<WsMessagesHandler>>>,
    ws_responses_dst: HashMap<String, Arc<RwLock<HashMap<String, oneshot::Sender<WsResult>>>>>,
}

impl GsbServices {
    pub fn bind(&mut self, components: HashSet<&str>, path: &str) -> Result<(), GsbApiError> {
        let ws_calls = self.ws_responders_map(path);
        match self.ws_requests_dst.get(path) {
            //TODO add msg
            None => return Err(GsbApiError::BadRequest),
            Some(addr) => {
                for component in components {
                    match component {
                        "GetMetadata" => {
                            log::info!("GetMetadata {path}");
                            <Self as ServiceBinder<GetMetadata>>::bind_service(
                                path,
                                component.to_string(),
                                ws_calls.clone(),
                                addr.clone(),
                            );
                        }
                        "GetChunk" => {
                            log::info!("GetChunk {path}");
                            <Self as ServiceBinder<GetChunk>>::bind_service(
                                path,
                                component.to_string(),
                                ws_calls.clone(),
                                addr.clone(),
                            );
                        }
                        _ => return Err(GsbApiError::BadRequest),
                    }
                }
            }
        }
        Ok(())
    }

    pub fn ws_responders_map(
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
        _component: String,
        senders_map: Arc<RwLock<HashMap<String, oneshot::Sender<WsResult>>>>,
        request_dst: Arc<Addr<WsMessagesHandler>>,
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
                request_dst.send(ws_request).await.unwrap();
                let ws_res = ws_receiver.await.map_err(Self::map_err)?;
                MSG::Item::try_from(ws_res)
            }
        });

        Ok(())
    }

    fn map_err(err: Canceled) -> MSG::Error;
}

impl ServiceBinder<GetMetadata> for GsbServices {
    fn map_err(err: Canceled) -> <GetMetadata as RpcMessage>::Error {
        gftp::Error::InternalError(format!("WS request failed: {}", err))
    }
}

impl TryInto<WsRequest> for (String, GetMetadata) {
    type Error = <GetMetadata as RpcMessage>::Error;

    fn try_into(self) -> Result<WsRequest, Self::Error> {
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

impl TryFrom<WsResult> for GftpMetadata {
    type Error = <GetMetadata as RpcMessage>::Error;

    fn try_from(res: WsResult) -> Result<Self, Self::Error> {
        match res.0 {
            Ok(ws_response) => {
                let response = serde_json::from_slice(&ws_response.payload)
                    .map_err(|err| ya_core_model::gftp::Error::InternalError(err.to_string()))?;
                Ok(response)
            }
            Err(err) => Err(ya_core_model::gftp::Error::InternalError(err.to_string())),
        }
    }
}

impl ServiceBinder<GetChunk> for GsbServices {
    fn map_err(err: Canceled) -> <GetChunk as RpcMessage>::Error {
        gftp::Error::InternalError(format!("WS request failed: {}", err))
    }
}

impl TryInto<WsRequest> for (String, GetChunk) {
    type Error = <GetChunk as RpcMessage>::Error;

    fn try_into(self) -> Result<WsRequest, Self::Error> {
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
                let response = serde_json::from_slice(&ws_response.payload)
                    .map_err(|err| ya_core_model::gftp::Error::InternalError(err.to_string()))?;
                Ok(response)
            }
            Err(err) => Err(ya_core_model::gftp::Error::InternalError(err.to_string())),
        }
    }
}
