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
use flexbuffers::{BuilderOptions, MapBuilder, MapReader, Reader};

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
                log::info!(
                    "Buffer: isAligned: {}: bitw: {:?}, buf: {:?}",
                    buffer.is_aligned(),
                    buffer.bitwidth(),
                    buffer.buffer()
                );
                let response = buffer.as_map();
                //TODO handle errors
                let id_r = response.index("id").unwrap();
                let id = id_r.as_str().to_string();
                log::info!(
                    "ID: {id}: isAligned: {}: bitw: {:?}, buf: {:?}",
                    id_r.is_aligned(),
                    id_r.bitwidth(),
                    id_r.buffer()
                );
                let payload_index = response.index("payload").unwrap();
                log::info!(
                    "Payload: isAligned: {}:  bitw: {:?}, buf: {:?}",
                    payload_index.is_aligned(),
                    payload_index.bitwidth(),
                    payload_index.buffer()
                );

                let payload_map = payload_index.as_map();
                let payload_fileSize = payload_map.index("fileSize").unwrap();
                log::info!(
                    "Payload fileSize: isAligned: {}:  bitw: {:?}, buf: {:?}",
                    id_r.is_aligned(),
                    payload_fileSize.bitwidth(),
                    payload_fileSize.buffer()
                );

                // let iter = payload_map.iter_values()

                let mut builder = flexbuffers::Builder::new(BuilderOptions::empty());

                let mut builder_map = builder.start_map();
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
        log::info!(
            "WS request (id: {}, component: {})",
            request.id,
            request.component
        );
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
    use std::{collections::{HashMap, BTreeSet}, fmt::Debug};

    use bytes::Bytes;
    use flexbuffers::{Buffer, Reader, FlexBufferType, BitWidth, BuilderOptions, Builder, MapBuilder, MapReader, VectorBuilder, VectorReader};
    use serde::{de::DeserializeOwned, Deserialize, Serialize};
    use serde_json::Value;
    use ya_core_model::gftp::GftpChunk;


    fn clone_map(mut b: MapBuilder, m_r: &MapReader<&[u8]>) -> Result<(), flexbuffers::ReaderError> {
        for key in m_r.iter_keys() {
            let v = m_r.index(key)?;
            match v.flexbuffer_type() {
                FlexBufferType::Null => b.push(key, ()),
                FlexBufferType::Int => b.push(key, v.as_i64()),
                FlexBufferType::UInt => b.push(key, v.as_u64()),
                FlexBufferType::Float => b.push(key, v.as_i64()),
                FlexBufferType::Bool => b.push(key, v.as_f64()),
                FlexBufferType::Key => b.push(key, v.as_str()),
                FlexBufferType::String => b.push(key, v.as_str()),
                FlexBufferType::IndirectInt => b.push(key, v.as_i64()),
                FlexBufferType::IndirectUInt => b.push(key, v.as_u64()),
                FlexBufferType::IndirectFloat => b.push(key, v.as_f64()),
                FlexBufferType::Map => clone_map(b.start_map(key), &v.as_map())?,
                FlexBufferType::Vector => clone_vector(b.start_vector(key), v.as_vector(), None)?,
                FlexBufferType::VectorInt => clone_vector(b.start_vector(key), v.as_vector(), Some(FlexBufferType::Int))?,
                FlexBufferType::VectorUInt => clone_vector(b.start_vector(key), v.as_vector(), Some(FlexBufferType::UInt))?,
                FlexBufferType::VectorFloat => clone_vector(b.start_vector(key), v.as_vector(), Some(FlexBufferType::Float))?,
                FlexBufferType::VectorKey => clone_vector(b.start_vector(key), v.as_vector(), Some(FlexBufferType::Key))?,
                FlexBufferType::VectorString => clone_vector(b.start_vector(key), v.as_vector(), Some(FlexBufferType::String))?,
                FlexBufferType::VectorBool => clone_vector(b.start_vector(key), v.as_vector(), Some(FlexBufferType::Bool))?,
                FlexBufferType::VectorInt2 => clone_vector(b.start_vector(key), v.as_vector(), Some(FlexBufferType::Int))?,
                FlexBufferType::VectorUInt2 => clone_vector(b.start_vector(key), v.as_vector(), Some(FlexBufferType::UInt))?,
                FlexBufferType::VectorFloat2 => clone_vector(b.start_vector(key), v.as_vector(), Some(FlexBufferType::Float))?,
                FlexBufferType::VectorInt3 => clone_vector(b.start_vector(key), v.as_vector(), Some(FlexBufferType::Int))?,
                FlexBufferType::VectorUInt3 => clone_vector(b.start_vector(key), v.as_vector(), Some(FlexBufferType::Float))?,
                FlexBufferType::VectorFloat3 => clone_vector(b.start_vector(key), v.as_vector(), Some(FlexBufferType::Float))?,
                FlexBufferType::VectorInt4 => clone_vector(b.start_vector(key), v.as_vector(), Some(FlexBufferType::Int))?,
                FlexBufferType::VectorUInt4 => clone_vector(b.start_vector(key), v.as_vector(), Some(FlexBufferType::Int))?,
                FlexBufferType::VectorFloat4 => clone_vector(b.start_vector(key), v.as_vector(), Some(FlexBufferType::Float))?,
                FlexBufferType::Blob => b.push(key, v.as_blob()),
            }
        }
        b.end_map();
        Ok(())
    }

    fn clone_vector(mut b: VectorBuilder, v_r: VectorReader<&[u8]>, t: Option<FlexBufferType>) ->  Result<(), flexbuffers::ReaderError> {
        for v in v_r.iter() {
            let typ = t.unwrap_or(v.flexbuffer_type());
            //TODO remove duplication
            match typ {
                FlexBufferType::Null => b.push(()),
                FlexBufferType::Int => b.push(v.as_i64()),
                FlexBufferType::UInt => b.push(v.as_u64()),
                FlexBufferType::Float => b.push(v.as_i64()),
                FlexBufferType::Bool => b.push(v.as_f64()),
                FlexBufferType::Key => b.push(v.as_str()),
                FlexBufferType::String => b.push(v.as_str()),
                FlexBufferType::IndirectInt => b.push(v.as_i64()),
                FlexBufferType::IndirectUInt => b.push(v.as_u64()),
                FlexBufferType::IndirectFloat => b.push(v.as_f64()),
                FlexBufferType::Map => clone_map(b.start_map(), &v.as_map())?,
                FlexBufferType::Vector => clone_vector(b.start_vector(), v.as_vector(), None)?,
                FlexBufferType::VectorInt => clone_vector(b.start_vector(), v.as_vector(), Some(FlexBufferType::Int))?,
                FlexBufferType::VectorUInt => clone_vector(b.start_vector(), v.as_vector(), Some(FlexBufferType::UInt))?,
                FlexBufferType::VectorFloat => clone_vector(b.start_vector(), v.as_vector(), Some(FlexBufferType::Float))?,
                FlexBufferType::VectorKey => clone_vector(b.start_vector(), v.as_vector(), Some(FlexBufferType::Key))?,
                FlexBufferType::VectorString => clone_vector(b.start_vector(), v.as_vector(), Some(FlexBufferType::String))?,
                FlexBufferType::VectorBool => clone_vector(b.start_vector(), v.as_vector(), Some(FlexBufferType::Bool))?,
                FlexBufferType::VectorInt2 => clone_vector(b.start_vector(), v.as_vector(), Some(FlexBufferType::Int))?,
                FlexBufferType::VectorUInt2 => clone_vector(b.start_vector(), v.as_vector(), Some(FlexBufferType::UInt))?,
                FlexBufferType::VectorFloat2 => clone_vector(b.start_vector(), v.as_vector(), Some(FlexBufferType::Float))?,
                FlexBufferType::VectorInt3 => clone_vector(b.start_vector(), v.as_vector(), Some(FlexBufferType::Int))?,
                FlexBufferType::VectorUInt3 => clone_vector(b.start_vector(), v.as_vector(), Some(FlexBufferType::Float))?,
                FlexBufferType::VectorFloat3 => clone_vector(b.start_vector(), v.as_vector(), Some(FlexBufferType::Float))?,
                FlexBufferType::VectorInt4 => clone_vector(b.start_vector(), v.as_vector(), Some(FlexBufferType::Int))?,
                FlexBufferType::VectorUInt4 => clone_vector(b.start_vector(), v.as_vector(), Some(FlexBufferType::Int))?,
                FlexBufferType::VectorFloat4 => clone_vector(b.start_vector(), v.as_vector(), Some(FlexBufferType::Float))?,
                FlexBufferType::Blob => b.push(v.as_blob()),
            }
        }
        b.end_vector();
        Ok(())
    }





    // fn to_pushable

    #[test]
    fn test_cloning() {
        let test_msg = DefaultMsg::default();
        // let test_msg_buf = flexbuffers::to_vec(test_msg).unwrap();
        let mut s = flexbuffers::FlexbufferSerializer::new();
        test_msg.serialize(&mut s).unwrap();
        let r = flexbuffers::Reader::get_root(s.view()).unwrap();
        let r_m = r.as_map();
        let r_m_p = r_m.index("payload").unwrap();
        let r_m_p_m = r_m_p.as_map();

        let mut builder = flexbuffers::Builder::new(BuilderOptions::empty());
        let mut builder_map = builder.start_map();
        let _ = clone_map(builder_map, &r_m_p_m).unwrap();

        println!("Copy: {:?}", builder.view());

        let r = Reader::get_root(builder.view()).unwrap();

        let cloned_payload = Payload::deserialize(r).unwrap();

        assert_eq!(test_msg.payload, cloned_payload);
    }

    // struct CustomSerializerMsg {
    //     id: String,
    //     payload: Vec<u8>,
    // }

    // impl<'de> Deserialize<'de> for CustomSerializerMsg {
    //     fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    //     where
    //         D: serde::Deserializer<'de>,
    //     {
    //         deserializer.deserialize_any(CustomVisitor {});
    //         todo!("fail")
    //     }
    // }

    // struct CustomVisitor {}

    // impl Visitor<'_> for CustomVisitor {
    //     type Value = CustomSerializerMsg;

    //     fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
    //         formatter.write_str("a custom key")
    //     }
        
    // }

    // #[test]
    // fn test_serde() {
    //     let m = Msg::default();
    //     let mut s = flexbuffers::FlexbufferSerializer::new();
    //     let _ = m.serialize(&mut s).unwrap();
    //     let r = flexbuffers::Reader::get_root(s.view()).unwrap();
    //     let x = CustomSerializerMsg::deserialize(r).unwrap();
    // }

    #[derive(Serialize, Deserialize, PartialEq, Debug, Default)]
    struct Payload {
        file_size: i64
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug, Default)]
    struct DefaultMsg {
        id: String,
        payload: Payload,
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug, Default)]
    struct ComplexMsg {
        id: String,
        payload: Payload,
        nested: DefaultMsg,
        other: i32,
    }

    #[test]
    fn test() {
        let m = DefaultMsg::default();
        find_payload(m);
    }

    #[test]
    fn test2() {
        let m = ComplexMsg::default();
        find_payload(m);
    }

    // fn find_payload<'de, MSG: Serialize + Deserialize<'de> + PartialEq + Debug>(msg: MSG) {
    fn find_payload<T: Serialize + DeserializeOwned + PartialEq + Default + Debug>(msg: T) {
        let mut s = flexbuffers::FlexbufferSerializer::new();
        msg.serialize(&mut s).unwrap();

        let r = flexbuffers::Reader::get_root(s.view()).unwrap();
        let r_m = r.as_map();
        let addr = r.address();
        let mut key_addresses = HashMap::new();
        let mut addresses = BTreeSet::new();
        for key in r_m.iter_keys() {
            let key_r = r_m.index(key).unwrap();
            let address = key_r.address();
            let typ = key_r.flexbuffer_type();
            let width = key_r.bitwidth();
            key_addresses.insert(key, (address, typ, width));
            addresses.insert(address);
        }
        let (payload_begin, payload_type, payload_bitwidth) = key_addresses.get("payload").unwrap();

        println!("Addresses: {:?}", addresses);
        addresses.split_off(payload_begin);
        let payload_end = addresses.pop_last().unwrap_or(0);

        let payload_buf = r.buffer().slice(payload_end..*payload_begin).unwrap().to_vec();
        let mut payload_buf = payload_buf.to_vec();

        payload_buf.extend([(*payload_type as u8) << 2 | *payload_bitwidth as u8]);
        payload_buf.extend([payload_bitwidth.n_bytes() as u8]);
        
        let root = Reader::get_root(&*payload_buf).unwrap();

        //]
        let test_msg = Payload::default();
        let test_msg_buf = flexbuffers::to_vec(test_msg).unwrap();
        println!("Manual: len: {}, {:?}", payload_buf.len(), payload_buf);
        println!("Auto:   len: {},  {:?}", test_msg_buf.len(), test_msg_buf);
        //

        let deserialized_msg = T::deserialize(root).unwrap();
        assert_eq!(msg, deserialized_msg);
        
    }


    
    fn find_payload_alt<T: Serialize + DeserializeOwned + PartialEq + Default + Debug>(msg: T) {
        let mut s = flexbuffers::FlexbufferSerializer::new();
        msg.serialize(&mut s).unwrap();

        let r = flexbuffers::Reader::get_root(s.view()).unwrap();
        let r_m = r.as_map();
        let addr = r.address();
        let mut key_addresses = HashMap::new();
        let mut addresses = BTreeSet::new();
        for key in r_m.iter_keys() {
            let key_r = r_m.index(key).unwrap();

            let key_r_m = key_r.as_map();

            let address = key_r.address();
            let typ = key_r.flexbuffer_type();
            let width = key_r.bitwidth();
            key_addresses.insert(key, (address, typ, width));
            addresses.insert(address);
        }
        let (payload_begin, payload_type, payload_bitwidth) = key_addresses.get("payload").unwrap();

        println!("Addresses: {:?}", addresses);
        addresses.split_off(payload_begin);
        let payload_end = addresses.pop_last().unwrap_or(0);

        let payload_buf = r.buffer().slice(payload_end..*payload_begin).unwrap().to_vec();
        let mut payload_buf = payload_buf.to_vec();

        payload_buf.extend([(*payload_type as u8) << 2 | *payload_bitwidth as u8]);
        payload_buf.extend([payload_bitwidth.n_bytes() as u8]);
        
        let root = Reader::get_root(&*payload_buf).unwrap();

        //

        let test_msg = Payload::default();
        let test_msg_buf = flexbuffers::to_vec(test_msg).unwrap();
        println!("Manual: len: {}, {:?}", payload_buf.len(), payload_buf);
        println!("Auto:   len: {},  {:?}", test_msg_buf.len(), test_msg_buf);
        //

        let deserialized_msg = T::deserialize(root).unwrap();
        assert_eq!(msg, deserialized_msg);
        
    }


    #[derive(Serialize, Deserialize, Debug, Default)]
    struct SerdeMsg {
        id: String,
        payload: serde_json::Value,
    }

    #[derive(Serialize, Deserialize, Debug, Default)]
    struct OrigMsg {
        id: String,
        payload: GftpChunk,
    }

    #[test]
    fn serde_test() {
        let chunk = ya_core_model::gftp::GftpChunk {
            offset: 10,
            content: vec![1,2,3,4]
        };
        let orig_msg = OrigMsg { id: "xxx".to_string(), payload: chunk };
        println!("Orig: {:?}", orig_msg);
        let ser = flexbuffers::to_vec(orig_msg).unwrap();

        let des: SerdeMsg = flexbuffers::from_slice(&ser).unwrap();
        println!("Serd: {:?}", des);
    }
}
