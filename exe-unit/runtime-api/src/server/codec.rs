use bytes::BytesMut;
use futures::{Sink, Stream};
use prost::Message;

use std::marker::PhantomData;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Decoder, Encoder};

pub struct Codec<M: Message> {
    _marker: PhantomData<M>,
}

impl<M: Message> Default for Codec<M> {
    fn default() -> Self {
        Codec::new()
    }
}

impl<M: Message> Codec<M> {
    fn new() -> Self {
        Codec {
            _marker: PhantomData,
        }
    }

    pub fn stream(output: impl AsyncRead) -> impl Stream<Item = Result<M, anyhow::Error>>
    where
        M: Default,
    {
        tokio_util::codec::FramedRead::new(output, Self::new())
    }

    pub fn sink(input: impl AsyncWrite) -> impl Sink<M, Error = anyhow::Error> {
        tokio_util::codec::FramedWrite::new(input, Self::new())
    }
}

impl<M: Message> Encoder<M> for Codec<M> {
    type Error = anyhow::Error;

    fn encode(&mut self, item: M, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let len = item.encoded_len();
        dst.reserve(len + prost::length_delimiter_len(len));
        Message::encode_length_delimited(&item, dst)?;
        Ok(())
    }
}

impl<M: Message + Default> Decoder for Codec<M> {
    type Item = M;
    type Error = anyhow::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 4 {
            src.reserve(10);
            return Ok(None);
        }
        let len = prost::decode_length_delimiter(src.clone())?;
        let len_size = prost::length_delimiter_len(len);
        let pending_len = len + len_size;
        if src.len() < pending_len {
            return Ok(None);
        }
        let dec = Message::decode_length_delimited(src)?;
        Ok(Some(dec))
    }
}
