mod api;
mod model;
mod service;
mod services;

use crate::service::{DropMessages, StartBuffering, StartRelaying};
use actix::prelude::*;
use actix::ActorFutureExt;
use actix::{Actor, Addr, Handler, StreamHandler};
use actix_http::ws::CloseCode;
use actix_http::ws::{CloseReason, ProtocolError};
use actix_web_actors::ws::{self, WebsocketContext};
use flexbuffers::{BuilderOptions, MapReader, Reader};
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use service::Service;
use services::Services;
use ya_service_api_interfaces::Provider;

pub const GSB_API_PATH: &str = "gsb-api/v1";

pub struct GsbApiService;

impl GsbApiService {
    pub async fn gsb<Context>(_: Context) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn rest<Context: Provider<Self, ()>>(ctx: &Context) -> actix_web::Scope {
        Self::rest_internal(ctx, crate::services::SERVICES.clone())
    }

    pub(crate) fn rest_internal<Context: Provider<Self, ()>>(
        _: &Context,
        services: Addr<Services>,
    ) -> actix_web::Scope {
        api::web_scope(services)
    }
}

pub(crate) type GsbError = ya_service_bus::Error;

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

impl WsResponse {
    pub(crate) fn try_new(
        response_key: &str,
        id: &str,
        payload: MapReader<&[u8]>,
    ) -> Result<WsResponse, String> {
        let mut response_builder = flexbuffers::Builder::new(BuilderOptions::empty());
        let mut response_map_builder = response_builder.start_map();
        let response_map_field_builder = response_map_builder.start_map(response_key);
        match flexbuffer_util::clone_map(response_map_field_builder, &payload) {
            Ok(_) => {
                response_map_builder.end_map();
                let response = WsResponse {
                    id: id.to_string(),
                    response: WsResponseMsg::Message(response_builder.view().to_vec()),
                };
                Ok(response)
            }
            Err(err) => Err(format!("Failed to read response payload. Err: {err}")),
        }
    }
}

#[derive(Debug)]
pub(crate) enum WsResponseMsg {
    Message(Vec<u8>),
    Error(GsbError),
}

impl WsResponseMsg {
    /// Close reason description
    fn desc(reason: &CloseReason, reason_name: &str) -> String {
        reason
            .description
            .clone()
            .map_or(reason_name.to_string(), |desc| {
                format!("{reason_name}: {desc}")
            })
    }
}

impl From<&DropMessages> for WsResponseMsg {
    fn from(value: &DropMessages) -> Self {
        let reason = &value.reason;
        let error = match reason.code {
            ws::CloseCode::Normal => GsbError::Closed(Self::desc(reason, "Normal")),
            ws::CloseCode::Away => GsbError::Closed(Self::desc(reason, "Away")),
            ws::CloseCode::Protocol => GsbError::Closed(Self::desc(reason, "Protocol")),
            ws::CloseCode::Unsupported => GsbError::Closed(Self::desc(reason, "Unsupported")),
            ws::CloseCode::Abnormal => GsbError::Closed(Self::desc(reason, "Abnormal")),
            ws::CloseCode::Invalid => GsbError::Closed(Self::desc(reason, "Invalid")),
            ws::CloseCode::Policy => GsbError::Closed(Self::desc(reason, "Policy")),
            ws::CloseCode::Size => GsbError::Closed(Self::desc(reason, "Size")),
            ws::CloseCode::Extension => GsbError::Closed(Self::desc(reason, "Extension")),
            ws::CloseCode::Error => GsbError::Closed(Self::desc(reason, "Error")),
            ws::CloseCode::Restart => GsbError::Closed(Self::desc(reason, "Restart")),
            ws::CloseCode::Again => GsbError::Closed(Self::desc(reason, "Again")),
            _ => GsbError::Closed(Self::desc(reason, "Other")),
        };
        WsResponseMsg::Error(error)
    }
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
    service: Addr<Service>,
}

impl WsMessagesHandler {
    pub fn handle(&mut self, buffer: bytes::Bytes, ctx: &mut WebsocketContext<WsMessagesHandler>) {
        match self.read_response(buffer) {
            Ok(ws_response) => {
                self.service
                    .send(ws_response)
                    .boxed()
                    .into_actor(self)
                    .map(|res, handler, ctx| {
                        if let Err(err) = res {
                            let desc = format!("Failed to send response. Err: {err}");
                            handler.close(ctx, CloseCode::Error, &desc)
                        }
                    })
                    .wait(ctx);
            }
            Err(err) => {
                let desc = format!("Failed to read response. Err: {err}");
                self.close(ctx, CloseCode::Policy, &desc)
            }
        }
    }

    pub fn read_response(&mut self, buffer: bytes::Bytes) -> Result<WsResponse, String> {
        let response =
            Reader::get_root(&*buffer).map_err(|err| format!("Missing root. Err: {err}"))?;
        let response = flexbuffer_util::as_map(&response, false)
            .map_err(|err| format!("Missing root map. Err: {err}"))?;
        let id = flexbuffer_util::read_string(&response, "id")
            .map_err(|err| format!("Missing response id. Err: {err}"))?;
        if let Ok(error_payload) = flexbuffer_util::read_map(&response, "error", false) {
            WsResponse::try_new("Err", &id, error_payload)
                .map_err(|err| format!("Failed to read error payload. Id: {id}. Err: {err}"))
        } else if let Ok(payload) = flexbuffer_util::read_map(&response, "payload", true) {
            WsResponse::try_new("Ok", &id, payload)
                .map_err(|err| format!("Failed to read payload. Id: {id}. Err: {err}"))
        } else {
            Err(format!("Missing 'payload' and 'error' fields. Id: {id}."))
        }
    }

    fn start_buffering(
        &self,
        reason: Option<CloseReason>,
        ctx: &mut WebsocketContext<WsMessagesHandler>,
    ) {
        log::debug!("WS Close. Reason: {reason:?}");
        let drop_messages = match reason {
            Some(reason) => DropMessages { reason },
            None => {
                let code = CloseCode::Normal;
                let description = Some("Closing".to_string());
                let reason = CloseReason { code, description };
                DropMessages { reason }
            }
        };
        let drop_messages_fut = self.service.send(drop_messages).boxed();
        let start_buffering_fut = self.service.send(StartBuffering).boxed();
        ctx.wait(actix::fut::wrap_future(async {
            if let Err(error) = drop_messages_fut.await {
                log::error!("Failed to send DropMessages. Err: {}", error);
            }
            if let Err(error) = start_buffering_fut.await {
                log::error!("Failed to send StartBuffering. Err: {}", error);
            }
        }));
    }

    fn close(&self, ctx: &mut WebsocketContext<WsMessagesHandler>, code: CloseCode, desc: &str) {
        let description = Some(desc.to_string());
        let reason = Some(CloseReason { code, description });
        self.start_buffering(reason.clone(), ctx);
        log::warn!("Closing WS handler: {}", desc);
        ctx.close(reason);
    }
}

impl Actor for WsMessagesHandler {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        log::debug!("WsMessagesHandler started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        log::debug!("WsMessagesHandler stopped");
    }
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
                    log::debug!("WS Binary (len {})", msg.len());
                    self.handle(msg, ctx);
                }
                ws::Message::Text(_) => {
                    self.close(ctx, CloseCode::Unsupported, "Text msg unsupported.")
                }
                ws::Message::Continuation(_) => {
                    self.close(ctx, CloseCode::Unsupported, "Continuation msg unsupported.")
                }
                ws::Message::Close(close_reason) => self.start_buffering(close_reason, ctx),
                ws::Message::Ping(message) => ctx.pong(&message),
                ws::Message::Pong(_) => log::warn!("Pong handling is not implemented."),
                ws::Message::Nop => log::warn!("Nop handling is not implemented."),
            },
            Err(cause) => ctx.close(Some(CloseReason {
                code: ws::CloseCode::Error,
                description: Some(format!("ProtocolError: {cause}")),
            })),
        };
    }

    fn started(&mut self, ctx: &mut Self::Context) {
        log::debug!("WS handler started.");
        self.service
            .send(StartRelaying {
                ws_handler: ctx.address(),
            })
            .into_actor(self)
            .map(|res, _, _ctx| {
                if let Err(err) = res {
                    log::error!("Failed to start buffering GSB messages. Err: {}", err);
                };
            })
            .spawn(ctx);
    }

    fn finished(&mut self, ctx: &mut Self::Context) {
        log::debug!("WS handler finished.");
        self.service
            .send(StartBuffering)
            .into_actor(self)
            .map(|res, _, ctx| {
                if let Err(err) = res {
                    log::error!("Failed to start buffering GSB messages. Err: {}", err);
                };
                log::debug!("Stopping WS handler actor.");
                ctx.stop();
            })
            .spawn(ctx);
    }
}

impl Handler<WsDisconnect> for WsMessagesHandler {
    type Result = <WsDisconnect as actix::Message>::Result;

    fn handle(&mut self, close: WsDisconnect, ctx: &mut Self::Context) -> Self::Result {
        log::debug!("Stopping WsMessagesHandler. Close reason: {:?}", close.0);
        ctx.close(Some(close.0));
    }
}

mod flexbuffer_util {
    use flexbuffers::{FlexBufferType, MapBuilder, MapReader, Pushable, Reader, VectorBuilder};

    trait FlexPusher<'b> {
        fn push<P: Pushable>(&mut self, p: P);
        fn start_map(&mut self) -> MapBuilder<'_>;
        fn start_vector(&mut self) -> VectorBuilder<'_>;
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
            self.builder.push(self.key, p)
        }

        fn start_map(&mut self) -> MapBuilder<'_> {
            self.builder.start_map(self.key)
        }

        fn start_vector(&mut self) -> VectorBuilder<'_> {
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

        fn start_map(&mut self) -> MapBuilder<'_> {
            self.builder.start_map()
        }

        fn start_vector(&mut self) -> VectorBuilder<'_> {
            self.builder.start_vector()
        }

        fn end(self) {
            self.builder.end_vector()
        }
    }

    pub(crate) fn read_string(
        reader: &MapReader<&[u8]>,
        key: &str,
    ) -> Result<String, anyhow::Error> {
        match reader.index(key) {
            Ok(field) => match field.get_str() {
                Ok(txt) => Ok(txt.to_string()),
                Err(err) => anyhow::bail!("Failed to read string field: {}. Err: {}", key, err),
            },
            Err(err) => anyhow::bail!("Failed to read field: {}. Err: {}", key, err),
        }
    }

    pub(crate) fn as_map<'a>(
        reader: &Reader<&'a [u8]>,
        allow_empty: bool,
    ) -> Result<MapReader<&'a [u8]>, anyhow::Error> {
        match reader.get_map() {
            Ok(map) => {
                if allow_empty || !map.is_empty() {
                    return Ok(map);
                }
                anyhow::bail!("Empty map");
            }
            Err(err) => anyhow::bail!("Failed to read map. Err: {}", err),
        }
    }

    pub(crate) fn read_map<'a>(
        reader: &MapReader<&'a [u8]>,
        key: &str,
        allow_empty: bool,
    ) -> Result<MapReader<&'a [u8]>, anyhow::Error> {
        match reader.index(key) {
            Ok(reader) => as_map(&reader, allow_empty),
            Err(err) => anyhow::bail!("Failed to find response field: {}. Err: {}", key, err),
        }
    }

    pub(crate) fn clone_map(
        builder: MapBuilder,
        map_reader: &MapReader<&[u8]>,
    ) -> Result<(), flexbuffers::ReaderError> {
        let mut pusher = FlexMapPusher { builder, key: "" };
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
            let v_type = value_type.unwrap_or_else(|| value.flexbuffer_type());
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
        use crate::flexbuffer_util::clone_map;
        use flexbuffers::{BuilderOptions, Reader};
        use serde::{de::DeserializeOwned, Deserialize, Serialize};
        use std::fmt::Debug;

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
            clone_map(builder_map, &r_m_p_m).unwrap();

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
