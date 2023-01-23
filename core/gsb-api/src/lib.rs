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
    use flexbuffers::{
        BitWidth, Buffer, Builder, BuilderOptions, FlexBufferType, MapBuilder, MapReader, Pushable,
        Reader, VectorBuilder, VectorReader,
    };
    use serde::{de::DeserializeOwned, Deserialize, Serialize};
    use std::{
        cell::RefCell,
        collections::{BTreeSet, HashMap},
        fmt::Debug,
    };

    trait FlexPusher<'b> {
        fn push<P: Pushable>(&mut self, p: P);
        fn start_map<'a>(&'a mut self) -> MapBuilder<'a>;
        fn start_vector<'a>(&'a mut self) -> VectorBuilder<'a>;
        fn end(self);
    }

    struct FlexMapPusher<'a> {
        builder: MapBuilder<'a>,
        key: &'a str,
    }

    impl<'a> FlexMapPusher<'a> {
        fn set_key(&mut self, key: &'a str) {
            self.key = key;
        }
    }

    impl<'a> FlexPusher<'a> for FlexMapPusher<'a> {
        fn push<P: Pushable>(&mut self, p: P) {
            self.builder.push(&self.key, p)
        }

        fn start_map<'b>(&'b mut self) -> MapBuilder<'b> {
            self.builder.start_map(self.key)
        }

        fn start_vector<'b>(&'b mut self) -> VectorBuilder<'b> {
            self.builder.start_vector(self.key)
        }

        fn end(self) {
            self.builder.end_map()
        }
    }

    struct FlexVecPusher<'a> {
        builder: VectorBuilder<'a>,
    }

    impl<'a> FlexPusher<'a> for FlexVecPusher<'a> {
        fn push<P: Pushable>(&mut self, p: P) {
            self.builder.push(p)
        }

        fn start_map<'b>(&'b mut self) -> MapBuilder<'b> {
            self.builder.start_map()
        }

        fn start_vector<'b>(&'b mut self) -> VectorBuilder<'b> {
            self.builder.start_vector()
        }

        fn end(self) {
            self.builder.end_vector()
        }
    }

    fn clone_map(
        builder: MapBuilder,
        map_reader: &MapReader<&[u8]>,
    ) -> Result<(), flexbuffers::ReaderError> {
        let mut pusher = FlexMapPusher {
            builder: builder,
            key: "",
        };
        for key in map_reader.iter_keys() {
            pusher.set_key(key);
            let value = map_reader.index(key)?;
            let value_type = value.flexbuffer_type();
            pusher = push(value, value_type, pusher)?;
        }
        pusher.end();
        Ok(())
    }

    fn clone_vector(
        builder: VectorBuilder,
        vector_reader: VectorReader<&[u8]>,
        value_type: Option<FlexBufferType>,
    ) -> Result<(), flexbuffers::ReaderError> {
        let mut pusher = FlexVecPusher { builder };
        for value in vector_reader.iter() {
            let v_type = value_type.unwrap_or(value.flexbuffer_type());
            pusher = push(value, v_type, pusher)?;
        }
        pusher.end();
        Ok(())
    }

    fn push<'r, 'b, B: FlexPusher<'b>>(
        value: Reader<&[u8]>,
        value_type: FlexBufferType,
        mut pusher: B,
    ) -> Result<B, flexbuffers::ReaderError> {
        match value_type {
            FlexBufferType::Null => pusher.push(()),
            FlexBufferType::Int => pusher.push(value.as_i64()),
            FlexBufferType::UInt => pusher.push(value.as_u64()),
            FlexBufferType::Float => pusher.push(value.as_i64()),
            FlexBufferType::Bool => pusher.push(value.as_f64()),
            FlexBufferType::Key => pusher.push(value.as_str()),
            FlexBufferType::String => pusher.push(value.as_str()),
            FlexBufferType::IndirectInt => pusher.push(value.as_i64()),
            FlexBufferType::IndirectUInt => pusher.push(value.as_u64()),
            FlexBufferType::IndirectFloat => pusher.push(value.as_f64()),
            FlexBufferType::Map => clone_map(pusher.start_map(), &value.as_map())?,
            FlexBufferType::Vector => clone_vector(pusher.start_vector(), value.as_vector(), None)?,

            FlexBufferType::VectorInt => clone_vector(
                pusher.start_vector(),
                value.as_vector(),
                Some(FlexBufferType::Int),
            )?,
            FlexBufferType::VectorUInt => clone_vector(
                pusher.start_vector(),
                value.as_vector(),
                Some(FlexBufferType::UInt),
            )?,
            FlexBufferType::VectorFloat => clone_vector(
                pusher.start_vector(),
                value.as_vector(),
                Some(FlexBufferType::Float),
            )?,
            FlexBufferType::VectorKey => clone_vector(
                pusher.start_vector(),
                value.as_vector(),
                Some(FlexBufferType::Key),
            )?,
            FlexBufferType::VectorString => clone_vector(
                pusher.start_vector(),
                value.as_vector(),
                Some(FlexBufferType::String),
            )?,
            FlexBufferType::VectorBool => clone_vector(
                pusher.start_vector(),
                value.as_vector(),
                Some(FlexBufferType::Bool),
            )?,
            FlexBufferType::VectorInt2 => clone_vector(
                pusher.start_vector(),
                value.as_vector(),
                Some(FlexBufferType::Int),
            )?,
            FlexBufferType::VectorUInt2 => clone_vector(
                pusher.start_vector(),
                value.as_vector(),
                Some(FlexBufferType::UInt),
            )?,
            FlexBufferType::VectorFloat2 => clone_vector(
                pusher.start_vector(),
                value.as_vector(),
                Some(FlexBufferType::Float),
            )?,
            FlexBufferType::VectorInt3 => clone_vector(
                pusher.start_vector(),
                value.as_vector(),
                Some(FlexBufferType::Int),
            )?,
            FlexBufferType::VectorUInt3 => clone_vector(
                pusher.start_vector(),
                value.as_vector(),
                Some(FlexBufferType::Float),
            )?,
            FlexBufferType::VectorFloat3 => clone_vector(
                pusher.start_vector(),
                value.as_vector(),
                Some(FlexBufferType::Float),
            )?,
            FlexBufferType::VectorInt4 => clone_vector(
                pusher.start_vector(),
                value.as_vector(),
                Some(FlexBufferType::Int),
            )?,
            FlexBufferType::VectorUInt4 => clone_vector(
                pusher.start_vector(),
                value.as_vector(),
                Some(FlexBufferType::Int),
            )?,
            FlexBufferType::VectorFloat4 => clone_vector(
                pusher.start_vector(),
                value.as_vector(),
                Some(FlexBufferType::Float),
            )?,
            FlexBufferType::Blob => pusher.push(value.as_blob()),
        }
        Ok(pusher)
    }

    #[test]
    fn test_complex() {
        let top = ComplexMsg {
            content: vec![1, 2, u16::MAX],
            id: "meh".to_string(),
            payload: Payload { file_size: 123123 },
            nested: DefaultMsg {
                id: "my_id".to_string(),
                payload: Payload { file_size: 3456456 },
            },
            other: i32::MIN,
        };
        let nested = &top.nested;
        let nested_name = "nested";
        test_cloning(&top, nested, nested_name)
    }

    #[test]
    fn test_default() {
        let top = DefaultMsg::default();
        let nested = &top.payload;
        let nested_name = "payload";
        test_cloning(&top, nested, nested_name)
    }

    fn test_cloning<
        TOP: Serialize + DeserializeOwned + PartialEq + Debug,
        NESTED: Serialize + DeserializeOwned + PartialEq + Debug,
    >(
        top: &TOP,
        nested: &NESTED,
        nested_name: &str,
    ) {
        let mut s = flexbuffers::FlexbufferSerializer::new();
        top.serialize(&mut s).unwrap();
        let r = flexbuffers::Reader::get_root(s.view()).unwrap();
        let r_m = r.as_map();
        let r_m_p = r_m.index(nested_name).unwrap();
        let r_m_p_m = r_m_p.as_map();

        let mut builder = flexbuffers::Builder::new(BuilderOptions::empty());
        let builder_map = builder.start_map();
        let _ = clone_map(builder_map, &r_m_p_m).unwrap();

        println!("Copy: {:?}", builder.view());

        let r = Reader::get_root(builder.view()).unwrap();

        let cloned_payload = NESTED::deserialize(r).unwrap();

        assert_eq!(nested, &cloned_payload);
    }

    // #[derive(Serialize, Deserialize)]
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
    //     let m = DefaultMsg::default();
    //     let mut s = flexbuffers::FlexbufferSerializer::new();
    //     let _ = m.serialize(&mut s).unwrap();
    //     let r = flexbuffers::Reader::get_root(s.view()).unwrap();
    //     let x = DefaultMsg::deserialize(r).unwrap();
    // }

    #[derive(Serialize, Deserialize, PartialEq, Debug, Default)]
    struct Payload {
        file_size: i64,
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug, Default)]
    struct DefaultMsg {
        id: String,
        payload: Payload,
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug, Default)]
    struct ComplexMsg {
        content: Vec<u16>,
        id: String,
        payload: Payload,
        nested: DefaultMsg,
        other: i32,
    }

    /*
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
     */
}
