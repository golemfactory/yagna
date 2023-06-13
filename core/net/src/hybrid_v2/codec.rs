use prost::Message;

use ya_core_model::NodeId;
use ya_sb_proto::codec::{GsbMessage, ProtocolError};
use ya_sb_proto::CallReplyCode;
use ya_service_bus::{Error, ResponseChunk};

pub(crate) fn encode_message(msg: GsbMessage) -> Result<Vec<u8>, Error> {
    let packet = ya_sb_proto::Packet { packet: Some(msg) };
    let len: usize = packet.encoded_len();

    let mut dst = Vec::with_capacity(4 + len);
    dst.extend((len as u32).to_be_bytes());
    packet
        .encode(&mut dst)
        .map_err(|e| Error::EncodingProblem(e.to_string()))?;

    Ok(dst)
}

pub(crate) fn decode_message(src: &[u8]) -> Result<Option<GsbMessage>, Error> {
    let msg_length = if src.len() < 4 {
        return Ok(None);
    } else {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(&src[0..4]);
        u32::from_be_bytes(buf) as usize
    };

    if src.len() < 4 + msg_length {
        return Ok(None);
    }

    let packet = ya_sb_proto::Packet::decode(&src[4..4 + msg_length])
        .map_err(|e| Error::EncodingProblem(e.to_string()))?;
    match packet.packet {
        Some(msg) => Ok(Some(msg)),
        None => Err(Error::EncodingProblem(
            ProtocolError::UnrecognizedMessageType.to_string(),
        )),
    }
}

pub(crate) fn decode_reply(data: Vec<u8>) -> Result<Vec<u8>, Error> {
    use std::convert::TryInto;

    let msg = match decode_message(data.as_slice()) {
        Ok(Some(packet)) => packet,
        _ => return Ok(data),
    };
    let reply = match msg {
        GsbMessage::CallReply(reply) => reply,
        _ => return Ok(data),
    };
    let code = match reply.code.try_into() {
        Ok(code) => code,
        _ => return Ok(data),
    };

    match code {
        CallReplyCode::CallReplyOk => Ok(data),
        CallReplyCode::CallReplyBadRequest => Err(Error::GsbBadRequest(
            String::from_utf8_lossy(&reply.data).to_string(),
        )),
        CallReplyCode::ServiceFailure => Err(Error::GsbFailure(
            String::from_utf8_lossy(&reply.data).to_string(),
        )),
    }
}

pub(crate) fn encode_request(
    caller: NodeId,
    address: String,
    request_id: String,
    data: Vec<u8>,
    no_reply: bool,
) -> anyhow::Result<Vec<u8>> {
    let message = GsbMessage::CallRequest(ya_sb_proto::CallRequest {
        caller: caller.to_string(),
        address,
        request_id,
        data,
        no_reply,
    });
    Ok(encode_message(message)?)
}

#[inline]
pub(crate) fn encode_reply(reply: ya_sb_proto::CallReply) -> anyhow::Result<Vec<u8>> {
    Ok(encode_message(GsbMessage::CallReply(reply))?)
}

pub(crate) fn encode_error(
    request_id: impl ToString,
    error: impl ToString,
    code: i32,
) -> anyhow::Result<Vec<u8>> {
    let message = GsbMessage::CallReply(ya_sb_proto::CallReply {
        request_id: request_id.to_string(),
        code,
        reply_type: ya_sb_proto::CallReplyType::Full as i32,
        data: error.to_string().into_bytes(),
    });
    Ok(encode_message(message)?)
}

pub(crate) fn reply_ok(request_id: impl ToString, chunk: ResponseChunk) -> GsbMessage {
    let reply_type = if chunk.is_full() {
        ya_sb_proto::CallReplyType::Full as i32
    } else {
        ya_sb_proto::CallReplyType::Partial as i32
    };

    GsbMessage::CallReply(ya_sb_proto::CallReply {
        request_id: request_id.to_string(),
        code: ya_sb_proto::CallReplyCode::CallReplyOk as i32,
        reply_type,
        data: chunk.into_bytes(),
    })
}

pub(crate) fn reply_err(request_id: impl ToString, err: impl ToString) -> GsbMessage {
    GsbMessage::CallReply(ya_sb_proto::CallReply {
        request_id: request_id.to_string(),
        code: ya_sb_proto::CallReplyCode::CallReplyBadRequest as i32,
        reply_type: ya_sb_proto::CallReplyType::Full as i32,
        data: err.to_string().into_bytes(),
    })
}

pub(crate) fn reply_eos(request_id: impl ToString) -> GsbMessage {
    GsbMessage::CallReply(ya_sb_proto::CallReply {
        request_id: request_id.to_string(),
        code: ya_sb_proto::CallReplyCode::CallReplyOk as i32,
        reply_type: ya_sb_proto::CallReplyType::Full as i32,
        data: vec![],
    })
}

#[cfg(test)]
mod tests {
    use std::iter::FromIterator;

    use crate::hybrid_v2::codec::{decode_message, encode_message};

    #[test]
    fn encode_message_compat() {
        use tokio_util::codec::Encoder;
        use ya_sb_proto::codec::GsbMessage;

        let msg = GsbMessage::CallReply(ya_sb_proto::CallReply {
            request_id: "10203040".to_string(),
            code: ya_sb_proto::CallReplyCode::CallReplyBadRequest as i32,
            reply_type: ya_sb_proto::CallReplyType::Full as i32,
            data: "err".to_string().into_bytes(),
        });
        let encoded = encode_message(msg.clone()).unwrap();

        let mut buf = bytes::BytesMut::with_capacity(msg.encoded_len());
        ya_sb_proto::codec::GsbMessageEncoder::default()
            .encode(msg.clone(), &mut buf)
            .unwrap();
        let encoded_orig = Vec::from_iter(buf.into_iter());

        assert_eq!(encoded_orig, encoded);
        assert_eq!(decode_message(encoded.as_slice()).unwrap().unwrap(), msg);
    }
}
