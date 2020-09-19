use std::convert::TryInto;
use std::mem::size_of;

use prost::Message;

use crate::gsb_api::*;
use crate::{MessageHeader, MessageType};
use thiserror::Error;

use tokio_util::codec::{Decoder, Encoder};

const MSG_HEADER_LENGTH: usize = size_of::<MessageHeader>();

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("Unrecognized message type: {0}")]
    UnrecognizedMessageType(i32),
    #[error("Cannot decode message header: not enough bytes")]
    HeaderNotEnoughBytes,
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("encode error: {0}")]
    Encode(#[from] prost::EncodeError),
    #[error("decode {0}")]
    Decode(#[from] prost::DecodeError),
    #[error("{0}")]
    RecvError(#[from] tokio::sync::mpsc::error::RecvError),
    #[error("packet too big")]
    MsgTooBig,
}

trait Encodable {
    // This trait exists because prost::Message has template methods

    fn encode_(&self, buf: &mut bytes::BytesMut) -> Result<(), ProtocolError>;
    fn encoded_len_(&self) -> usize;
}

impl<T: Message> Encodable for T {
    fn encode_(&self, mut buf: &mut bytes::BytesMut) -> Result<(), ProtocolError> {
        Ok(self.encode(&mut buf)?)
    }

    fn encoded_len_(&self) -> usize {
        self.encoded_len()
    }
}

#[derive(Debug)]
pub enum GsbMessage {
    RegisterRequest(RegisterRequest),
    RegisterReply(RegisterReply),
    UnregisterRequest(UnregisterRequest),
    UnregisterReply(UnregisterReply),
    CallRequest(CallRequest),
    CallReply(CallReply),
    SubscribeRequest(SubscribeRequest),
    SubscribeReply(SubscribeReply),
    UnsubscribeRequest(UnsubscribeRequest),
    UnsubscribeReply(UnsubscribeReply),
    BroadcastRequest(BroadcastRequest),
    BroadcastReply(BroadcastReply),
    Ping,
    Pong,
}

impl GsbMessage {
    fn unpack(self) -> (MessageType, Box<dyn Encodable>) {
        match self {
            GsbMessage::RegisterRequest(msg) => (MessageType::RegisterRequest, Box::new(msg)),
            GsbMessage::RegisterReply(msg) => (MessageType::RegisterReply, Box::new(msg)),
            GsbMessage::UnregisterRequest(msg) => (MessageType::UnregisterRequest, Box::new(msg)),
            GsbMessage::UnregisterReply(msg) => (MessageType::UnregisterReply, Box::new(msg)),
            GsbMessage::CallRequest(msg) => (MessageType::CallRequest, Box::new(msg)),
            GsbMessage::CallReply(msg) => (MessageType::CallReply, Box::new(msg)),
            GsbMessage::SubscribeRequest(msg) => (MessageType::SubscribeRequest, Box::new(msg)),
            GsbMessage::SubscribeReply(msg) => (MessageType::SubscribeReply, Box::new(msg)),
            GsbMessage::UnsubscribeRequest(msg) => (MessageType::UnsubscribeRequest, Box::new(msg)),
            GsbMessage::UnsubscribeReply(msg) => (MessageType::UnsubscribeReply, Box::new(msg)),
            GsbMessage::BroadcastRequest(msg) => (MessageType::BroadcastRequest, Box::new(msg)),
            GsbMessage::BroadcastReply(msg) => (MessageType::BroadcastReply, Box::new(msg)),
            GsbMessage::Ping => (MessageType::Ping, Box::new(Ping {})),
            GsbMessage::Pong => (MessageType::Pong, Box::new(Pong {})),
        }
    }
}

impl Into<GsbMessage> for RegisterRequest {
    fn into(self) -> GsbMessage {
        GsbMessage::RegisterRequest(self)
    }
}

impl Into<GsbMessage> for RegisterReply {
    fn into(self) -> GsbMessage {
        GsbMessage::RegisterReply(self)
    }
}

impl Into<GsbMessage> for UnregisterRequest {
    fn into(self) -> GsbMessage {
        GsbMessage::UnregisterRequest(self)
    }
}

impl Into<GsbMessage> for UnregisterReply {
    fn into(self) -> GsbMessage {
        GsbMessage::UnregisterReply(self)
    }
}

impl Into<GsbMessage> for CallRequest {
    fn into(self) -> GsbMessage {
        GsbMessage::CallRequest(self)
    }
}

impl Into<GsbMessage> for CallReply {
    fn into(self) -> GsbMessage {
        GsbMessage::CallReply(self)
    }
}

impl Into<GsbMessage> for SubscribeRequest {
    fn into(self) -> GsbMessage {
        GsbMessage::SubscribeRequest(self)
    }
}

impl Into<GsbMessage> for SubscribeReply {
    fn into(self) -> GsbMessage {
        GsbMessage::SubscribeReply(self)
    }
}

impl Into<GsbMessage> for UnsubscribeRequest {
    fn into(self) -> GsbMessage {
        GsbMessage::UnsubscribeRequest(self)
    }
}

impl Into<GsbMessage> for UnsubscribeReply {
    fn into(self) -> GsbMessage {
        GsbMessage::UnsubscribeReply(self)
    }
}

impl Into<GsbMessage> for BroadcastRequest {
    fn into(self) -> GsbMessage {
        GsbMessage::BroadcastRequest(self)
    }
}

impl Into<GsbMessage> for BroadcastReply {
    fn into(self) -> GsbMessage {
        GsbMessage::BroadcastReply(self)
    }
}

impl Into<GsbMessage> for Ping {
    fn into(self) -> GsbMessage {
        GsbMessage::Ping
    }
}

impl Into<GsbMessage> for Pong {
    fn into(self) -> GsbMessage {
        GsbMessage::Pong
    }
}

fn decode_header(src: &mut bytes::BytesMut) -> Result<Option<MessageHeader>, ProtocolError> {
    if src.len() < MSG_HEADER_LENGTH {
        Ok(None)
    } else {
        let buf = src.split_to(MSG_HEADER_LENGTH);
        Ok(Some(MessageHeader::decode(buf)?))
    }
}

fn decode_message(
    src: &mut bytes::BytesMut,
    header: &MessageHeader,
) -> Result<Option<GsbMessage>, ProtocolError> {
    let msg_length = header
        .msg_length
        .try_into()
        .map_err(|_| ProtocolError::MsgTooBig)?;
    if src.len() < msg_length {
        Ok(None)
    } else {
        let buf = src.split_to(msg_length);
        let msg_type = MessageType::from_i32(header.msg_type);
        let msg: GsbMessage = match msg_type {
            Some(MessageType::RegisterRequest) => RegisterRequest::decode(buf.as_ref())?.into(),
            Some(MessageType::RegisterReply) => RegisterReply::decode(buf.as_ref())?.into(),
            Some(MessageType::UnregisterRequest) => UnregisterRequest::decode(buf.as_ref())?.into(),
            Some(MessageType::UnregisterReply) => UnregisterReply::decode(buf.as_ref())?.into(),
            Some(MessageType::CallRequest) => CallRequest::decode(buf.as_ref())?.into(),
            Some(MessageType::CallReply) => CallReply::decode(buf.as_ref())?.into(),
            Some(MessageType::SubscribeRequest) => SubscribeRequest::decode(buf.as_ref())?.into(),
            Some(MessageType::SubscribeReply) => SubscribeReply::decode(buf.as_ref())?.into(),
            Some(MessageType::UnsubscribeRequest) => {
                UnsubscribeRequest::decode(buf.as_ref())?.into()
            }
            Some(MessageType::UnsubscribeReply) => UnsubscribeReply::decode(buf.as_ref())?.into(),
            Some(MessageType::BroadcastRequest) => BroadcastRequest::decode(buf.as_ref())?.into(),
            Some(MessageType::BroadcastReply) => BroadcastReply::decode(buf.as_ref())?.into(),
            Some(MessageType::Ping) => Ping::decode(buf.as_ref())?.into(),
            Some(MessageType::Pong) => Pong::decode(buf.as_ref())?.into(),
            None => return Err(ProtocolError::UnrecognizedMessageType(header.msg_type)),
        };
        Ok(Some(msg))
    }
}

fn encode_message(dst: &mut bytes::BytesMut, msg: GsbMessage) -> Result<(), ProtocolError> {
    let (msg_type, msg) = msg.unpack();
    encode_message_unpacked(dst, msg_type, msg.as_ref())?;
    Ok(())
}

fn encode_message_unpacked(
    dst: &mut bytes::BytesMut,
    msg_type: MessageType,
    msg: &dyn Encodable,
) -> Result<(), ProtocolError> {
    let msg_type = msg_type as i32;
    let msg_length = msg.encoded_len_() as u32;
    let header = MessageHeader {
        msg_type,
        msg_length,
    };
    header.encode(dst);
    msg.encode_(dst)?;
    Ok(())
}

#[derive(Default)]
pub struct GsbMessageDecoder {
    msg_header: Option<MessageHeader>,
}

impl GsbMessageDecoder {
    pub fn new() -> Self {
        GsbMessageDecoder { msg_header: None }
    }
}

impl Decoder for GsbMessageDecoder {
    type Item = GsbMessage;
    type Error = ProtocolError;

    fn decode(&mut self, src: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if self.msg_header == None {
            self.msg_header = decode_header(src)?;
        }
        match &self.msg_header {
            None => Ok(None),
            Some(header) => match decode_message(src, &header)? {
                None => {
                    src.reserve(header.msg_length as usize);
                    Ok(None)
                }
                Some(msg) => {
                    self.msg_header = None;
                    Ok(Some(msg))
                }
            },
        }
    }
}

#[derive(Default)]
pub struct GsbMessageEncoder;

impl Encoder for GsbMessageEncoder {
    type Item = GsbMessage;
    type Error = ProtocolError;

    fn encode(&mut self, item: Self::Item, dst: &mut bytes::BytesMut) -> Result<(), Self::Error> {
        encode_message(dst, item)
    }
}

#[derive(Default)]
pub struct GsbMessageCodec {
    encoder: GsbMessageEncoder,
    decoder: GsbMessageDecoder,
}

impl Encoder for GsbMessageCodec {
    type Item = GsbMessage;
    type Error = ProtocolError;

    fn encode(&mut self, item: Self::Item, dst: &mut bytes::BytesMut) -> Result<(), Self::Error> {
        self.encoder.encode(item, dst)
    }
}

impl Decoder for GsbMessageCodec {
    type Item = GsbMessage;
    type Error = ProtocolError;

    fn decode(&mut self, src: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.decoder.decode(src)
    }
}
