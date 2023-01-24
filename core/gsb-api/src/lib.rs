mod api;
mod services;

use actix::prelude::*;
use actix::{dev::MessageResponse, Actor, Addr, Handler, MailboxError, StreamHandler};
use actix_http::{
    ws::{CloseReason, ProtocolError},
    StatusCode,
};
use actix_web::ResponseError;
use actix_web_actors::ws;
use bytes::{Buf, Bytes};
use flexbuffers::{BuilderOptions, Reader};
use serde::{Deserialize, Serialize};
use serde_json::json;
use services::AService;
use thiserror::Error;
use ya_service_api_interfaces::Provider;
use ya_service_bus::serialization;

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
    async fn handle(service: Addr<AService>, buffer: bytes::Bytes) {
        match Reader::get_root(&*buffer) {
            Ok(response) => {
                match response.flexbuffer_type() {
                    flexbuffers::FlexBufferType::Map => {
                        let response = response.as_map();
                        let id = response.index("id").unwrap();
                        let id = id.as_str().to_string();
                        let payload = response.index("payload").unwrap();
                        let payload = payload.as_map();
                        
                        let mut payload_builder = flexbuffers::Builder::new(BuilderOptions::empty());
                        let payload_map_builder = payload_builder.start_map();
                        nested_flexbuffer::clone_map(payload_map_builder, &payload).unwrap();
                        
                        let mut top_builder = flexbuffers::Builder::new(BuilderOptions::empty());
                        let mut top_map_builder = top_builder.start_map();
                        top_map_builder.push("Ok", payload_builder.view());
                        top_map_builder.end_map();
                        
                        let response = WsResponse {
                            id,
                            response: WsResponseMsg::Message(top_builder.view().to_vec()),
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
                    },
                    _ => {
                        todo!()
                    }
                }
                // let id_r = response.index("id").unwrap();
                // let id = id_r.as_str().to_string();
                // log::info!(
                //     "ID: {id}: isAligned: {}: bitw: {:?}, buf: {:?}",
                //     id_r.is_aligned(),
                //     id_r.bitwidth(),
                //     id_r.buffer()
                // );
                // let payload_index = response.index("payload").unwrap();
                // log::info!(
                //     "Payload: isAligned: {}:  bitw: {:?}, buf: {:?}",
                //     payload_index.is_aligned(),
                //     payload_index.bitwidth(),
                //     payload_index.buffer()
                // );

                // let payload_map = payload_index.as_map();
                // let payload_fileSize = payload_map.index("fileSize").unwrap();
                // log::info!(
                //     "Payload fileSize: isAligned: {}:  bitw: {:?}, buf: {:?}",
                //     id_r.is_aligned(),
                //     payload_fileSize.bitwidth(),
                //     payload_fileSize.buffer()
                // );

                // // let iter = payload_map.iter_values()

                // let mut builder = flexbuffers::Builder::new(BuilderOptions::empty());

                // let builder_map = builder.start_map();
                // // for x in payload_map.iter_keys() {
                // //     let f=  payload_map.index(x).unwrap();
                // //     builder_map.push(x, f.buffer());
                // // }
                // builder_map.end_map();
                // let payload = builder.view();

                // let mut b = flexbuffers::Builder::new(BuilderOptions::empty());
                // let mut map_b = b.start_map();
                // map_b.push("Ok", payload);
                // map_b.end_map();
                // let payload = b.view();

                // let response = WsResponse {
                //     id,
                //     response: WsResponseMsg::Message(payload.to_vec()),
                // };

                // log::info!("WsResponse: {}", response.id);
                // match service.send(response).await {
                //     Ok(res) => {
                //         if let Err(err) = res {
                //             log::error!("Failed to handle WS msg: {err}");
                //             //TODO error response?
                //         }
                //     }
                //     Err(err) => {
                //         log::error!("Internal error: {err}");
                //         //TODO error response?
                //     }
                // }
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

mod nested_flexbuffer {
    use flexbuffers::{
        FlexBufferType, MapBuilder, MapReader, Pushable, Reader, VectorBuilder,
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

    pub (crate) fn clone_map(
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

    fn clone_vec<'a, P: FlexPusher<'a>>(
        flex_pusher: &mut P,
        reader: Reader<&[u8]>,
        value_type: FlexBufferType,
    ) -> Result<(), flexbuffers::ReaderError> {
        clone_vec_optional_type(flex_pusher, reader, Some(value_type))
    }

    fn clone_vec_untyped<'a, P: FlexPusher<'a>>(
        flex_pusher: &mut P,
        reader: Reader<&[u8]>,
    ) -> Result<(), flexbuffers::ReaderError> {
        clone_vec_optional_type(flex_pusher, reader, None)
    }

    fn clone_vec_optional_type<'a, P: FlexPusher<'a>>(
        flex_pusher: &mut P,
        reader: Reader<&[u8]>,
        value_type: Option<FlexBufferType>,
    ) -> Result<(), flexbuffers::ReaderError> {
        let builder = flex_pusher.start_vector();
        let vector_reader = reader.get_vector()?;
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
            FlexBufferType::Int => pusher.push(value.get_i64()?),
            FlexBufferType::UInt => pusher.push(value.get_u64()?),
            FlexBufferType::Float => pusher.push(value.get_f64()?),
            FlexBufferType::Bool => pusher.push(value.get_bool()?),
            FlexBufferType::Key => pusher.push(value.get_key()?),
            FlexBufferType::String => pusher.push(value.get_str()?),
            FlexBufferType::IndirectInt => pusher.push(value.get_i64()?),
            FlexBufferType::IndirectUInt => pusher.push(value.get_u64()?),
            FlexBufferType::IndirectFloat => pusher.push(value.get_f64()?),
            FlexBufferType::Map => clone_map(pusher.start_map(), &value.get_map()?)?,
            FlexBufferType::Vector => clone_vec_untyped(&mut pusher, value)?,
            FlexBufferType::VectorInt => clone_vec(&mut pusher, value, FlexBufferType::Int)?,
            FlexBufferType::VectorUInt => clone_vec(&mut pusher, value, FlexBufferType::UInt)?,
            FlexBufferType::VectorFloat => clone_vec(&mut pusher, value, FlexBufferType::Float)?,
            FlexBufferType::VectorKey => clone_vec(&mut pusher, value, FlexBufferType::Key)?,
            FlexBufferType::VectorString => clone_vec(&mut pusher, value, FlexBufferType::String)?,
            FlexBufferType::VectorBool => clone_vec(&mut pusher, value, FlexBufferType::Bool)?,
            FlexBufferType::VectorInt2 => clone_vec(&mut pusher, value, FlexBufferType::Int)?,
            FlexBufferType::VectorUInt2 => clone_vec(&mut pusher, value, FlexBufferType::UInt)?,
            FlexBufferType::VectorFloat2 => clone_vec(&mut pusher, value, FlexBufferType::Float)?,
            FlexBufferType::VectorInt3 => clone_vec(&mut pusher, value, FlexBufferType::Int)?,
            FlexBufferType::VectorUInt3 => clone_vec(&mut pusher, value, FlexBufferType::UInt)?,
            FlexBufferType::VectorFloat3 => clone_vec(&mut pusher, value, FlexBufferType::Float)?,
            FlexBufferType::VectorInt4 => clone_vec(&mut pusher, value, FlexBufferType::Int)?,
            FlexBufferType::VectorUInt4 => clone_vec(&mut pusher, value, FlexBufferType::UInt)?,
            FlexBufferType::VectorFloat4 => clone_vec(&mut pusher, value, FlexBufferType::Float)?,
            FlexBufferType::Blob => pusher.push(value.get_blob()?),
        }
        Ok(pusher)
    }

    #[cfg(test)]
    mod nested_flexbuffer_tests {
        use std::fmt::Debug;

        use flexbuffers::{Reader, BuilderOptions};
        use serde::{de::DeserializeOwned, Deserialize, Serialize};

        use crate::nested_flexbuffer::clone_map;


        #[test]
        fn test_deserialization() {
            let payload = Payload { file_size: 11 };            
            let signed_msg = DefaultMsg { id: "123123".to_string(), payload };
            let mut s = flexbuffers::FlexbufferSerializer::new();
            signed_msg.serialize(&mut s).unwrap();

            let r = flexbuffers::Reader::get_root(s.view()).unwrap();
            let r_m = r.as_map();
            let r_m = r_m.index("payload").unwrap();
            let r_m = r_m.as_map();

            let mut builder = flexbuffers::Builder::new(BuilderOptions::empty());
            let builder_map = builder.start_map();
            clone_map(builder_map, &r_m).unwrap();

            let r = Reader::get_root(builder.view()).unwrap();
            let payload = Payload::deserialize(r).unwrap();
            assert_eq!(11, payload.file_size);

            //
            
            let mut test_builder = flexbuffers::Builder::new(BuilderOptions::empty());
            let mut test_map_builder = test_builder.start_map();
            test_map_builder.push("Ok", builder.view());
            test_map_builder.end_map();

            //

            let test_payload = ya_service_bus::serialization::from_slice::<Result<Payload,()>>(test_builder.view()).unwrap().unwrap();
            assert_eq!(payload, test_payload);
        }

        #[test]
        fn test_json() {
            let json_payload: serde_json::Value = serde_json::json!({
                    "file_size": 11
                });
            let mut s = flexbuffers::FlexbufferSerializer::new();
            json_payload.serialize(&mut s).unwrap();
            let r = flexbuffers::Reader::get_root(s.view()).unwrap();
            let r_m = r.as_map();

            let mut builder = flexbuffers::Builder::new(BuilderOptions::empty());
            let builder_map = builder.start_map();
            clone_map(builder_map, &r_m).unwrap();

            let r = Reader::get_root(builder.view()).unwrap();
            let payload = Payload::deserialize(r).unwrap();

            assert_eq!(11, payload.file_size);
        }

        
        #[test]
        fn test_signed_unsigned() {
            let signed_payload = SignedPayload { file_size: 11 };            
            let signed_msg = SignedDefaultMsg { id: "123123".to_string(), payload: signed_payload };
            let mut s = flexbuffers::FlexbufferSerializer::new();
            signed_msg.serialize(&mut s).unwrap();

            let r = flexbuffers::Reader::get_root(s.view()).unwrap();
            let r_m = r.as_map();
            let r_m = r_m.index("payload").unwrap();
            let r_m = r_m.as_map();

            let mut builder = flexbuffers::Builder::new(BuilderOptions::empty());
            let builder_map = builder.start_map();
            clone_map(builder_map, &r_m).unwrap();

            let r = Reader::get_root(builder.view()).unwrap();
            let payload = Payload::deserialize(r).unwrap();
            assert_eq!(11, payload.file_size);
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

        #[derive(Serialize, Deserialize, PartialEq, Debug, Default)]
        struct Payload {
            file_size: u64,
        }
        
        #[derive(Serialize, Deserialize, PartialEq, Debug, Default)]
        struct SignedPayload {
            file_size: i64,
        }

        #[derive(Serialize, Deserialize, PartialEq, Debug, Default)]
        struct SignedDefaultMsg {
            id: String,
            payload: SignedPayload,
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
    }
}
