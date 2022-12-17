//! Provider side operations
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    future::Future,
    marker::PhantomData,
    sync::{Arc, Mutex, RwLock},
    vec,
};

use actix::{Actor, StreamHandler};
use actix_http::{
    ws::{CloseReason, Item, ProtocolError},
    StatusCode,
};
use actix_web::{web, App, Error, HttpRequest, HttpResponse, HttpServer, Responder};
use actix_web_actors::ws;
use lazy_static::{lazy_static, __Deref};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use serde_json::json;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::{typed as bus, RpcEndpoint, RpcMessage};

use crate::{GsbApiError, WsRequest, WsResponse, WS_CALL};

lazy_static! {
    pub(crate) static ref SERVICES: Arc<Mutex<GsbServices>> =
        Arc::new(Mutex::new(GsbServices::default()));
}

// type MSG = Box<dyn RpcMessage<Item = Box<dyn Serialize + 'static + Sync + Send>, Error = Box<dyn Serialize + 'static + Sync + Send>>>;
// // type CALLER = ;
// type MSG_FUT = Future<Output = Result<MSG, anyhow::Error>>;

// // trait Caller: FnMut(String, MSG) -> Future<Output = Result<MSG, anyhow::Error>> + 'static {}
// type CALLER = Box<dyn FnMut(String, MSG) -> MSG_FUT>;

// trait GsbCaller {
//     type REQ: RpcMessage + Into<WsRequest>;
//     type RES: RpcMessage + From<WsResponse>;

//     fn call(self, path: String, req: Self::REQ) -> Future<Output=Result<Self::RES, <<Self as GsbCaller>::RES as RpcMessage>::Error >>;
// }

trait GsbCaller {
    fn call<REQ: RpcMessage + Into<WsRequest>, RES: RpcMessage + From<WsResponse>>(self, path: String, req: REQ) -> Future<Output=Result<RES, RES::Error>>;
}

struct GsbCallerEnabled {

}

// impl GsbCaller for GsbCallerEnabled {
//     fn call<REQ: RpcMessage + Into<WsRequest>, RES: RpcMessage + From<WsResponse>>(self, path: String, req: REQ) -> Future<Output=Result<RES, RES::Error>> {
//         todo!()
//     }
// }

// #[derive(Default)]
// struct GsbCaller<REQ, RES>
// where
//     REQ: RpcMessage + Into<WsRequest>,
//     RES: RpcMessage + From<WsResponse>,
// {
//     req_type: PhantomData<REQ>,
//     res_type: PhantomData<RES>,
//     ws_caller: Option<Arc<Mutex<WS_CALL>>>,
// }

// impl<REQ, RES> GsbCaller<REQ, RES>
// where
//     REQ: RpcMessage + Into<WsRequest>,
//     RES: RpcMessage + From<WsResponse>,
// {
//     async fn call(self, path: String, req: REQ) -> Result<REQ, REQ::Error> {
//         if let Some(ws_caller) = self.ws_caller {
//             let ws_caller = ws_caller.lock().unwrap();
//             ws_caller.as_mut()
//         }
//         let ws_req: WsRequest = req.into();
//         todo!("NYI")
//     }
// }

type OPTIONAL_WS_CALL = Arc<RwLock<Option<WS_CALL>>>;

struct GsbMessage;

#[derive(Default)]
pub(crate) struct GsbServices {
    callers: HashMap<String, HashMap<String,OPTIONAL_WS_CALL>>,
}

impl GsbServices {
    pub fn bind(&mut self, components: HashSet<String>, path: String) -> Result<(), GsbApiError> {
        for component in components {
            match component.as_str() {
                "GetMetadata" => {
                    log::info!("GetMetadata {path}");
                }
                "GetChunk" => {
                    log::info!("GetChunk {path}");
                }
                _ => return Err(GsbApiError::BadRequest),
            }
        }
        Ok(())
    }

    fn bind_service<MSG>(&mut self, path: String, id: String)
    where 
        MSG: RpcMessage + Into<WsRequest>, 
        MSG::Item: From<WsResponse>
    {
        let ws_call_pointer = self.ws_call_pointer(&path, &id);
        let _ = bus::bind_with_caller(&path, 
            move |path, packet: MSG| async move 
            {
                let ws_call = ws_call_pointer.read().unwrap();
                let ws_request = packet.into();
                if let Some(ws_call) = ws_call.as_deref_mut() {
                    match ws_call.call(path, ws_request).await {
                        Ok(res) => Ok(MSG::Item::from(res)),
                        Err(_err) => todo!("Error"),
                    }
                } else {
                    todo!("Not initialised")
                }
            
        });
        todo!()
    }

    fn ws_call_pointer(&mut self, path: &str, id: &str) -> OPTIONAL_WS_CALL {
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
                let caller = Arc::new(RwLock::new(None));
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
            Ok(msg) => {
                match msg {
                    ws::Message::Text(msg) => {
                        log::info!("Text: {:?}", msg);
                        match serde_json::from_slice::<WsRequest>(msg.as_bytes()) {
                            Ok(request) => {
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
