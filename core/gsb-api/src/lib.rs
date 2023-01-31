mod api;
mod service;
mod services;

use actix::prelude::*;
use actix::{Actor, Addr, Handler, MailboxError, StreamHandler};
use actix_http::{
    ws::{CloseReason, ProtocolError},
    StatusCode,
};
use actix_web::{HttpResponse, ResponseError};
use actix_web_actors::ws;
use flexbuffers::{BuilderOptions, Reader};
use serde::{Deserialize, Serialize};
use serde_json::json;
use service::Service;
use services::{BindError, FindError, UnbindError};
use thiserror::Error;
use ya_client_model::ErrorMessage;
use ya_service_api_interfaces::Provider;

pub const GSB_API_PATH: &str = "gsb-api/v1";

pub struct GsbApiService;

impl GsbApiService {
    pub async fn gsb<Context>(_: Context) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn rest<Context: Provider<Self, ()>>(_: &Context) -> actix_web::Scope {
        api::web_scope()
    }
}

#[derive(Error, Debug)]
pub(crate) enum GsbApiError {
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Internal error: {0}")]
    InternalError(String),
}

impl From<BindError> for GsbApiError {
    fn from(error: BindError) -> Self {
        match error {
            BindError::DuplicatedService(_) => Self::BadRequest(error.to_string()),
            BindError::InvalidService(_) => Self::BadRequest(error.to_string()),
        }
    }
}

impl From<UnbindError> for GsbApiError {
    fn from(error: UnbindError) -> Self {
        match error {
            UnbindError::ServiceNotFound(_) => Self::NotFound(error.to_string()),
            UnbindError::InvalidService(_) => Self::BadRequest(error.to_string()),
            UnbindError::UnbindFailed(_) => Self::InternalError(error.to_string()),
        }
    }
}

impl From<FindError> for GsbApiError {
    fn from(error: FindError) -> Self {
        match error {
            FindError::EmptyAddress => Self::BadRequest(error.to_string()),
            FindError::ServiceNotFound(_) => Self::NotFound(error.to_string()),
        }
    }
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
        GsbApiError::InternalError(format!("Serialization error {value}"))
    }
}

impl From<actix_web::Error> for GsbApiError {
    fn from(value: actix_web::Error) -> Self {
        GsbApiError::InternalError(format!("Actix error: {value}"))
    }
}

impl ResponseError for GsbApiError {
    fn status_code(&self) -> StatusCode {
        match *self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> actix_web::HttpResponse<actix_http::body::BoxBody> {
        match self {
            GsbApiError::BadRequest(message) => {
                HttpResponse::BadRequest().json(ErrorMessage::new(message))
            }
            GsbApiError::NotFound(message) => {
                HttpResponse::NotFound().json(ErrorMessage::new(message))
            }
            GsbApiError::InternalError(message) => {
                HttpResponse::InternalServerError().json(ErrorMessage::new(message))
            }
        }
    }
}

#[derive(Message, Serialize, Deserialize, Debug)]
#[rtype(result = "Result<(), anyhow::Error>")]
struct WsRequest {
    id: String,
    component: String,
    payload: Vec<u8>,
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
    Error(ya_service_bus::Error),
}

#[derive(Debug, Serialize, Deserialize)]
struct Msg {
    id: String,
    payload: serde_json::Value,
}

#[derive(Message, Debug)]
#[rtype(result = "()")]
struct WsDisconnect(CloseReason);

pub(crate) struct WsMessagesHandler {
    // pub responders: Arc<RwLock<HashMap<String, Sender<WsResult>>>>,
    service: Addr<Service>,
}

impl WsMessagesHandler {
    async fn handle(service: Addr<Service>, buffer: bytes::Bytes) {
        match Reader::get_root(&*buffer) {
            Ok(response) => {
                match response.flexbuffer_type() {
                    flexbuffers::FlexBufferType::Map => {
                        let response = response.as_map();
                        let id = response.index("id").unwrap(); //TODO handle it
                        let id = id.as_str().to_string();
                        let payload = response.index("payload").unwrap(); //TODO handle it
                        let payload = payload.as_map();

                        let mut response_builder =
                            flexbuffers::Builder::new(BuilderOptions::empty());
                        let mut ok_payload_map_builder = response_builder.start_map();
                        let payload_map_builder = ok_payload_map_builder.start_map("Ok"); //TODO on Err put here Error+msg
                        flexbuffer_util::clone_map(payload_map_builder, &payload).unwrap(); //TODO handle it
                        ok_payload_map_builder.end_map();

                        let response = WsResponse {
                            id,
                            response: WsResponseMsg::Message(response_builder.view().to_vec()),
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
                    _ => {
                        todo!()
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
        let mut request_builder = flexbuffers::Builder::new(BuilderOptions::empty());
        let mut request_map_builder = request_builder.start_map();
        request_map_builder.push("id", &*request.id);
        request_map_builder.push("component", &*request.component);
        let payload_map_builder = request_map_builder.start_map("payload");

        let payload = Reader::get_root(&*request.payload).unwrap(); //TODO handle error
        let payload_map = payload.as_map(); //TODO check type before as_map
        flexbuffer_util::clone_map(payload_map_builder, &payload_map).unwrap(); //TODO handle error
        request_map_builder.end_map();
        ctx.binary(request_builder.view().to_vec());
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

impl Handler<WsDisconnect> for WsMessagesHandler {
    type Result = <WsDisconnect as actix::Message>::Result;

    fn handle(&mut self, close: WsDisconnect, ctx: &mut Self::Context) -> Self::Result {
        ctx.close(Some(close.0));
        ctx.stop(); //TODO necessary?
    }
}

mod flexbuffer_util {
    use flexbuffers::{FlexBufferType, MapBuilder, MapReader, Pushable, Reader, VectorBuilder};

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

    pub(crate) fn clone_map(
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
        pusher: &mut P,
        reader: Reader<&[u8]>,
        value_type: FlexBufferType,
    ) -> Result<(), flexbuffers::ReaderError> {
        clone_vec_optional_type(pusher, reader, Some(value_type))
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
            #[allow(deprecated)]
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
    mod tests {
        use std::fmt::Debug;

        use flexbuffers::{BuilderOptions, Reader};
        use serde::{de::DeserializeOwned, Deserialize, Serialize};

        use crate::flexbuffer_util::clone_map;

        #[test]
        fn test_deserialization() {
            let payload = Payload { file_size: 11 };
            let signed_msg = DefaultMsg {
                id: "123123".to_string(),
                payload,
            };
            let mut s = flexbuffers::FlexbufferSerializer::new();
            signed_msg.serialize(&mut s).unwrap();

            let r = flexbuffers::Reader::get_root(s.view()).unwrap();
            let r_m = r.as_map();
            let r_m = r_m.index("payload").unwrap();
            let r_m = r_m.as_map();

            let mut builder = flexbuffers::Builder::new(BuilderOptions::empty());
            let mut top_builder_map = builder.start_map();
            let builder_map = top_builder_map.start_map("Ok");
            clone_map(builder_map, &r_m).unwrap();
            top_builder_map.end_map();

            let test_payload =
                ya_service_bus::serialization::from_slice::<Result<Payload, ()>>(builder.view())
                    .unwrap()
                    .unwrap();
            assert_eq!(Payload { file_size: 11 }, test_payload);
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
            let signed_msg = SignedDefaultMsg {
                id: "123123".to_string(),
                payload: signed_payload,
            };
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
