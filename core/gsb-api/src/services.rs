//! Provider side operations
use crate::{GsbApiError, WsCall, WsRequest, WsResponse, WsResult, WS_CALL};
use actix::{Actor, StreamHandler};
use actix_http::ws::{CloseReason, ProtocolError};
use actix_web_actors::ws;
use lazy_static::lazy_static;
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    convert::{TryFrom, TryInto},
    fmt::Debug,
    future::Future,
    pin::Pin,
    result::Result::{Err, Ok},
    sync::{Arc, Mutex, RwLock},
};
use ya_core_model::gftp::{GetChunk, GetMetadata, GftpChunk, GftpMetadata};
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
    callers: HashMap<String, HashMap<String, Arc<RwLock<WS_CALL>>>>,
}

impl GsbServices {
    pub fn bind(&mut self, components: HashSet<&str>, path: &str) -> Result<(), GsbApiError> {
        for component in components {
            match component {
                "GetMetadata" => {
                    log::info!("GetMetadata {path}");
                    self.bind_service::<GetMetadata>(path, component.to_string());
                }
                "GetChunk" => {
                    log::info!("GetChunk {path}");
                }
                _ => return Err(GsbApiError::BadRequest),
            }
        }
        std::result::Result::Ok(())
    }

    fn bind_service<'a, MSG>(&mut self, path: &str, component: String)
    where
        MSG: RpcMessage,
        (String, MSG): TryInto<WsRequest, Error = <MSG as RpcMessage>::Error>,
        MSG::Item: TryFrom<WsResult, Error = <MSG as RpcMessage>::Error>,
    {
        let ws_call_pointer = self.ws_call_pointer(&path, &component);
        let _ = bus::bind_with_caller(&path, move |path, packet: MSG| {
            let ws_call_pointer = ws_call_pointer.clone();
            let component = component.to_string().clone();
            let path = path.clone();
            async move {
                let ws_request = (component, packet).try_into()?;
                let ws_call = ws_call_pointer.read().unwrap();
                let ws_res = ws_call.call(path, ws_request).await;
                MSG::Item::try_from(ws_res)
            }
        });
        todo!()
    }

    fn ws_call_pointer(&mut self, path: &str, id: &str) -> Arc<RwLock<WS_CALL>> {
        let id_callers = match self.callers.get_mut(path) {
            Some(id_callers) => id_callers,
            None => {
                let id_callers = HashMap::new();
                self.callers.insert(path.to_string(), id_callers);
                self.callers.get_mut(path).unwrap()
            }
        };
        let caller_ref = match id_callers.get_mut(id) {
            Some(callers) => callers,
            None => {
                let caller: Arc<RwLock<Box<dyn WsCall + Send + Sync + 'static>>> =
                    Arc::new(RwLock::new(Box::new(UnboundWsCall {})));
                id_callers.insert(id.to_string(), caller);
                id_callers.get_mut(id).unwrap()
            }
        };
        caller_ref.clone()
    }
}

pub(crate) struct WsMessagesHandler {
    pub services: Arc<Mutex<GsbServices>>,
}

impl Actor for WsMessagesHandler {
    type Context = ws::WebsocketContext<Self>;
}

impl StreamHandler<Result<actix_http::ws::Message, ProtocolError>> for WsMessagesHandler {
    fn handle(
        &mut self,
        item: Result<actix_http::ws::Message, ProtocolError>,
        ctx: &mut Self::Context,
    ) {
        match item {
            std::result::Result::Ok(msg) => {
                match msg {
                    ws::Message::Text(msg) => {
                        log::info!("Text: {:?}", msg);
                        match serde_json::from_slice::<WsRequest>(msg.as_bytes()) {
                            std::result::Result::Ok(request) => {
                                log::info!("WsRequest: {request:?}");
                            }
                            Err(err) => todo!("NYI Deserialization error: {err:?}"),
                        }
                    }
                    ws::Message::Binary(msg) => {
                        todo!("NYI Binary: {:?}", msg);
                    }
                    ws::Message::Continuation(msg) => {
                        todo!("NYI Continuation: {:?}", msg);
                    }
                    ws::Message::Close(msg) => {
                        log::info!("Close: {:?}", msg);
                    }
                    ws::Message::Ping(msg) => {
                        log::info!("Ping: {:?}", msg);
                    }
                    any => todo!("NYI support of: {:?}", any),
                }
                ctx.text(
                    json!({
                      "id": "myId",
                      "component": "GetMetadata",
                      "payload": "payload",
                    })
                    .to_string(),
                )
            }
            Err(cause) => ctx.close(Some(CloseReason {
                code: ws::CloseCode::Error,
                description: Some(
                    json!({
                        "cause": cause.to_string()
                    })
                    .to_string(),
                ),
            })),
        };
    }
}

#[derive(Debug)]
pub struct UnboundWsCall;

impl WsCall for UnboundWsCall {
    fn call(&self, _path: String, _request: WsRequest) -> Pin<Box<dyn Future<Output = WsResult>>> {
        todo!("Unbound Call NYI")
    }
}

impl TryInto<WsRequest> for (String, GetMetadata) {
    type Error = <GetMetadata as RpcMessage>::Error;

    fn try_into(self) -> Result<WsRequest, Self::Error> {
        let payload = serde_json::to_vec(&self)
            .map_err(|err| ya_core_model::gftp::Error::InternalError(err.to_string()))?;
        let component = self.0;
        let id = GetMetadata::ID.to_string();
        std::result::Result::Ok(WsRequest {
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

impl TryInto<WsRequest> for (String, GetChunk) {
    type Error = <GetChunk as RpcMessage>::Error;

    fn try_into(self) -> Result<WsRequest, Self::Error> {
        let payload = serde_json::to_vec(&self)
            .map_err(|err| ya_core_model::gftp::Error::InternalError(err.to_string()))?;
        let component = self.0;
        let id = GetMetadata::ID.to_string();
        std::result::Result::Ok(WsRequest {
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
