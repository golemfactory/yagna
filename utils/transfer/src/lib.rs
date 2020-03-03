pub mod error;
pub mod file;
pub mod hash;
pub mod http;

use crate::error::{ChannelError, Error};
use crate::hash::*;
use bytes::Bytes;
use futures::channel::mpsc::{channel, Receiver, Sender};
use futures::channel::oneshot;
use futures::future::{AbortHandle, AbortRegistration};
use futures::task::{Context, Poll};
use futures::{Sink, Stream, StreamExt};
use std::pin::Pin;
use url::Url;

pub async fn transfer<S, T>(stream: S, mut sink: TransferSink<T, Error>) -> Result<(), Error>
where
    S: Stream<Item = Result<T, Error>>,
{
    let res_rx = sink.res_rx.take().unwrap();
    stream.forward(sink).await?;
    res_rx
        .await
        .map_err(ChannelError::from)
        .map_err(Error::from)
        .map(|_| ())
}

pub trait AbortableStream<T, E>: Stream<Item = std::result::Result<T, E>> {
    fn abort_handle(&self) -> AbortHandle;
}

#[derive(Clone, Debug)]
pub enum TransferData {
    Bytes(Bytes),
}

impl TransferData {
    pub fn to_bytes(&self) -> &Bytes {
        match &self {
            TransferData::Bytes(b) => b,
        }
    }

    pub fn into_bytes(self) -> Bytes {
        match self {
            TransferData::Bytes(b) => b,
        }
    }
}

impl From<Bytes> for TransferData {
    fn from(b: Bytes) -> Self {
        TransferData::Bytes(b)
    }
}

pub trait TransferProvider<T, E> {
    fn schemes(&self) -> Vec<&'static str>;

    fn source(&self, url: &Url) -> TransferStream<T, E>;
    fn destination(&self, url: &Url) -> TransferSink<T, E>;
}

pub struct TransferStream<T, E> {
    rx: Receiver<Result<T, E>>,
    pub abort_handle: AbortHandle,
}

pub struct StreamHandles<T, E> {
    pub tx: Sender<Result<T, E>>,
    pub abort_reg: AbortRegistration,
}

impl<T, E> TransferStream<T, E> {
    pub fn create(channel_size: usize) -> (Self, StreamHandles<T, E>) {
        let (tx, rx) = channel(channel_size);
        let (abort_handle, abort_reg) = AbortHandle::new_pair();
        (
            TransferStream { rx, abort_handle },
            StreamHandles { tx, abort_reg },
        )
    }
}

impl<T, E> Stream for TransferStream<T, E> {
    type Item = Result<T, E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Stream::poll_next(Pin::new(&mut self.rx), cx)
    }
}

impl<T, E> AbortableStream<T, E> for TransferStream<T, E> {
    fn abort_handle(&self) -> AbortHandle {
        self.abort_handle.clone()
    }
}

pub struct TransferSink<T, E> {
    tx: Sender<Result<T, E>>,
    pub res_rx: Option<oneshot::Receiver<Result<(), E>>>,
}

pub struct SinkHandles<T, E> {
    pub rx: Receiver<Result<T, E>>,
    pub res_tx: oneshot::Sender<Result<(), E>>,
}

impl<T, E> TransferSink<T, E> {
    pub fn create(channel_size: usize) -> (Self, SinkHandles<T, E>) {
        let (tx, rx) = channel(channel_size);
        let (res_tx, res_rx) = oneshot::channel();
        (
            TransferSink {
                tx,
                res_rx: Some(res_rx),
            },
            SinkHandles { rx, res_tx },
        )
    }
}

impl<T> Sink<T> for TransferSink<T, Error> {
    type Error = Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Sink::poll_ready(Pin::new(&mut self.tx), cx).map_err(Error::from)
    }

    fn start_send(mut self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        Sink::start_send(Pin::new(&mut self.tx), Ok(item)).map_err(Error::from)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Sink::poll_flush(Pin::new(&mut self.tx), cx).map_err(Error::from)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Sink::poll_close(Pin::new(&mut self.tx), cx).map_err(Error::from)
    }
}

pub struct HashStream<T, E, S>
where
    S: Stream<Item = Result<T, E>>,
{
    inner: S,
    hasher: Box<dyn Hasher>,
    hash: Vec<u8>,
    result: Option<Vec<u8>>,
}

impl<T, S> HashStream<T, Error, S>
where
    S: Stream<Item = Result<T, Error>> + Unpin,
{
    pub fn try_new(stream: S, alg: &str, hash: Vec<u8>) -> Result<Self, Error> {
        let hasher: Box<dyn Hasher> = match alg {
            "sha3" => match hash.len() * 8 {
                224 => Box::new(Sha3_224::default()),
                256 => Box::new(Sha3_256::default()),
                384 => Box::new(Sha3_384::default()),
                512 => Box::new(Sha3_512::default()),
                len => {
                    return Err(Error::UnsupportedDigestError(format!(
                        "Unsupported digest {} of length {}: {}",
                        alg,
                        len,
                        hex::encode(&hash),
                    )))
                }
            },
            _ => {
                return Err(Error::UnsupportedDigestError(format!(
                    "Unsupported digest: {}",
                    alg
                )))
            }
        };

        Ok(HashStream {
            inner: stream,
            hasher,
            hash,
            result: None,
        })
    }
}

impl<S> Stream for HashStream<TransferData, Error, S>
where
    S: Stream<Item = Result<TransferData, Error>> + Unpin,
{
    type Item = Result<TransferData, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let result = Stream::poll_next(Pin::new(&mut self.inner), cx);

        if let Poll::Ready(ref opt) = result {
            match opt {
                Some(item) => {
                    if let Ok(data) = item {
                        self.hasher.input(data.to_bytes());
                    }
                }
                None => {
                    let result = match &self.result {
                        Some(r) => r,
                        None => {
                            self.result = Some(self.hasher.result());
                            self.result.as_ref().unwrap()
                        }
                    };

                    if &self.hash != result {
                        return Poll::Ready(Some(Err(Error::InvalidHashError {
                            expected: hex::encode(&self.hash),
                            hash: hex::encode(&result),
                        })));
                    }
                }
            }
        }

        result
    }
}

impl<S> AbortableStream<TransferData, Error> for HashStream<TransferData, Error, S>
where
    S: AbortableStream<TransferData, Error> + Unpin,
{
    fn abort_handle(&self) -> AbortHandle {
        self.inner.abort_handle()
    }
}
