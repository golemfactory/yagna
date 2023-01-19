mod api;
mod services;

use std::convert::TryFrom;
use std::ops::DerefMut;
use std::str::Utf8Error;

use actix::prelude::*;
use actix::{dev::MessageResponse, Actor, Addr, Handler, MailboxError, StreamHandler};
use actix_http::{
    ws::{CloseReason, ProtocolError},
    StatusCode,
};
use actix_web::ResponseError;
use actix_web_actors::ws;

use bytes::{Buf, Bytes};
use flexbuffers::{Reader, BuilderOptions, MapReader, MapBuilder};

use lazy_static::__Deref;
use serde::{Deserialize, Serialize};
use serde_json::json;
use services::AService;

use thiserror::Error;
use ya_service_api_interfaces::Provider;

pub const GSB_API_PATH: &str = "/gsb-api/v1";

pub struct GsbApiService;

impl GsbApiService {
    pub async fn gsb<Context>(_: Context) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn rest<Context: Provider<Self, ()>>(_ctx: &Context) -> actix_web::Scope {
        api::web_scope()
    }
}

#[derive(Error, Debug)]
enum GsbApiError {
    //TODO add msg
    #[error("Bad request")]
    BadRequest,
    //TODO add msg
    #[error("Internal error")]
    InternalError(String),
    #[error(transparent)]
    Any(#[from] anyhow::Error),
}

impl From<ya_service_bus::Error> for GsbApiError {
    fn from(value: ya_service_bus::Error) -> Self {
        GsbApiError::InternalError(format!("GSB error: {value}"))
    }
}

impl From<MailboxError> for GsbApiError {
    fn from(value: MailboxError) -> Self {
        GsbApiError::InternalError(format!("Actix error: {value}"))
    }
}

impl From<serde_json::Error> for GsbApiError {
    fn from(value: serde_json::Error) -> Self {
        GsbApiError::InternalError(format!("Serde error {value}"))
    }
}

impl From<actix_web::Error> for GsbApiError {
    fn from(value: actix_web::Error) -> Self {
        GsbApiError::InternalError(format!("Actix error: {value}"))
    }
}

#[derive(Error, Debug)]
enum WsApiError {
    #[error("Internal Error")]
    InternalError,
    #[error(transparent)]
    Any(#[from] anyhow::Error),
}

impl ResponseError for GsbApiError {
    fn status_code(&self) -> StatusCode {
        match *self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Message, Serialize, Deserialize, Debug)]
#[rtype(result = "Result<(), anyhow::Error>")]
struct WsRequest {
    id: String,
    component: String,
    msg: Vec<u8>,
}

#[derive(Deserialize)]
struct WsResponseBody {
    pub id: String,
    pub payload: Vec<u8>,
}

#[derive(Message, Debug)]
#[rtype(result = "Result<(), anyhow::Error>")]
pub(crate) struct WsResponse {
    pub id: String,
    pub response: WsResponseMsg,
}

#[derive(Debug)]
pub(crate) enum WsResponseMsg {
    Message(Vec<u8>),
    Error(GsbApiError),
}

#[derive(Debug, Serialize, Deserialize)]
struct Msg {
    id: String,
    payload: serde_json::Value,
}

pub(crate) struct WsMessagesHandler {
    // pub responders: Arc<RwLock<HashMap<String, Sender<WsResult>>>>,
    service: Addr<AService>,
}


#[derive(Default)]
struct MyBuffer {
    pub b: Bytes,
}

impl flexbuffers::Buffer for MyBuffer {
    type BufferString = String;

    fn slice(&self, range: std::ops::Range<usize>) -> Option<Self> {
        log::info!("Slice {:?} of {} bytes buffer", range, self.b.len());
        if range.start > range.end || range.end > self.b.len() {
            None
        } else {
            let b = self.b.slice(range);
            Some(Self { b })
        }
    }

    fn empty() -> Self {
        log::info!("Default buffer");
        Self::default()
    }

    fn buffer_str(&self) -> Result<Self::BufferString, std::str::Utf8Error> {
        let str = std::str::from_utf8(self.b.chunk()).map(str::to_string);
        log::info!("Buffer str {:?}", str);
        str
    }

    fn shallow_copy(&self) -> Self {
        log::info!("Shallow copy");
        self.slice(0..self.len()).unwrap()
    }

    fn empty_str() -> Self::BufferString {
        log::info!("Empty str");
        Self::empty().buffer_str().unwrap()
    }
}

impl core::ops::Deref for MyBuffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.b.deref()
    }
}

impl WsMessagesHandler {
    async fn handle(service: Addr<AService>, msg: bytes::Bytes) {
        // let buf = MyBuffer { b: msg };
        match Reader::get_root(&*msg) {
            Ok(buffer) => {
                // buffer.get_s
                log::info!("Buffer: isAligned: {}: bitw: {:?}, buf: {:?}", buffer.is_aligned(), buffer.bitwidth(), buffer.buffer());
                let response = buffer.as_map();
                //TODO handle errors
                let id_r = response.index("id").unwrap();
                let id = id_r.as_str().to_string();
                log::info!("ID: {id}: isAligned: {}: bitw: {:?}, buf: {:?}", id_r.is_aligned(), id_r.bitwidth(), id_r.buffer());
                let payload_index = response.index("payload").unwrap();
                log::info!("Payload: isAligned: {}:  bitw: {:?}, buf: {:?}", payload_index.is_aligned(), payload_index.bitwidth(), payload_index.buffer());
                
                let payload_map = payload_index.as_map();
                let payload_fileSize = payload_map.index("fileSize").unwrap();
                log::info!("Payload fileSize: isAligned: {}:  bitw: {:?}, buf: {:?}", id_r.is_aligned(), payload_fileSize.bitwidth(), payload_fileSize.buffer());

                // let iter = payload_map.iter_values()

                let mut builder = flexbuffers::Builder::new(BuilderOptions::empty());

                let mut builder_map= builder.start_map();
                // for x in payload_map.iter_keys() {
                //     let f=  payload_map.index(x).unwrap();
                //     builder_map.push(x, f.buffer());
                // }
                builder_map.end_map();
                let payload = builder.view();


                let mut b = flexbuffers::Builder::new(BuilderOptions::empty());
                let mut map_b = b.start_map();
                map_b.push("Ok", payload);
                map_b.end_map();
                let payload = b.view();

                
                let response = WsResponse {
                    id,
                    response: WsResponseMsg::Message(payload.to_vec()),
                };


                log::info!("WsResponse: {}", response.id);
                match service.send(response).await {
                    Ok(res) => {
                        if let Err(err) = res {
                            log::error!("Failed to handle WS msg: {err}");
                            //TODO error response?
                        }
                    }
                    Err(err) => {
                        log::error!("Internal error: {err}");
                        //TODO error response?
                    }
                }
            }
            Err(err) => {
                //TODO shutdown service connections?
                log::error!("WS response error: {err}");
            }
        }
    }
}

impl Actor for WsMessagesHandler {
    type Context = ws::WebsocketContext<Self>;
}

impl Handler<WsRequest> for WsMessagesHandler {
    type Result = <WsRequest as Message>::Result;

    fn handle(&mut self, request: WsRequest, ctx: &mut Self::Context) -> Self::Result {
        log::info!("WS request (id: {}, component: {})", request.id, request.component);
        let msg = flexbuffers::to_vec(&request)
            .map_err(|err| anyhow::anyhow!("Failed to serialize msg: {}", err))?;
        ctx.binary(msg);
        Ok(())
    }
}

impl StreamHandler<Result<actix_http::ws::Message, ProtocolError>> for WsMessagesHandler {
    fn handle(
        &mut self,
        item: Result<actix_http::ws::Message, ProtocolError>,
        ctx: &mut Self::Context,
    ) {
        match item {
            Ok(msg) => match msg {
                ws::Message::Binary(msg) => {
                    log::info!("Binary (len {})", msg.len());
                    let service = self.service.clone();
                    ctx.spawn(actix::fut::wrap_future(Self::handle(service, msg)));
                }
                ws::Message::Text(msg) => {
                    log::info!("Text (len {})", msg.len());
                    let service = self.service.clone();
                    ctx.spawn(actix::fut::wrap_future(Self::handle(
                        service,
                        msg.into_bytes(),
                    )));
                }
                ws::Message::Continuation(msg) => {
                    todo!("NYI Continuation: {:?}", msg);
                }
                ws::Message::Close(msg) => {
                    log::info!("Close: {:?}", msg);
                }
                ws::Message::Ping(msg) => {
                    log::info!("Ping (len {})", msg.len());
                }
                any => todo!("NYI support of: {:?}", any),
            },
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


#[cfg(test)]
mod nested_flexbuffer {
    use serde::{Serialize, Deserialize};

    #[derive(Serialize, Deserialize, Debug, Default)]
    struct NestedMsg {
        file_size: i64,
    }

    #[derive(Serialize, Deserialize, Debug, Default)]
    struct Msg {
        id: String,
        payload: NestedMsg,
    }

    #[test]
    fn test() {
        let m = Msg::default();
        let mut s = flexbuffers::FlexbufferSerializer::new();
        m.serialize(&mut s).unwrap();
    
        let r = flexbuffers::Reader::get_root(s.view()).unwrap();
        let r_m = r.as_map();
        let mut key_addresses = Vec::new();
        for key in r_m.iter_keys() {
            let key_r = r_m.index(key).unwrap();
            let address = key_r.address();
            key_addresses.push((key, address));
        };
        //TODO build flexbuffer for "payload"
        let s = r.get_str().unwrap();
    }
}