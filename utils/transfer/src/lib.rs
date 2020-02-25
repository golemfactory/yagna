pub mod error;
pub mod file;
pub mod http;

use crate::error::Error;
use bytes::Bytes;
use futures::channel::mpsc::{channel, Receiver, Sender};
use futures::future::LocalBoxFuture;
use futures::task::{Context, Poll};
use futures::{Sink, Stream, StreamExt};
use std::pin::Pin;

pub async fn transfer<T, R>(
    stream: TransferStream<T, Error>,
    mut sink: TransferSink<T, R, Error>,
) -> Result<Option<R>, Error> {
    let sink_fut = sink.take_future();

    stream.forward(sink).await?;

    match sink_fut {
        Some(fut) => Ok(Some(fut.await?)),
        None => Ok(None),
    }
}

#[derive(Clone, Debug)]
enum TransferData {
    Bytes(Bytes),
}

impl TransferData {
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

pub trait TransferProvider<T, R, E> {
    fn supports(url: &String) -> bool;

    fn source(self, url: String) -> TransferStream<T, E>;
    fn destination(self, url: String) -> TransferSink<T, R, E>;
}

pub struct TransferStream<T, E> {
    rx: Receiver<Result<T, E>>,
}

impl<T, E> TransferStream<T, E> {
    pub fn create(channel_size: usize) -> (Self, Sender<Result<T, E>>) {
        let (tx, rx) = channel(channel_size);
        (TransferStream { rx }, tx)
    }
}

impl<T, E> Stream for TransferStream<T, E> {
    type Item = Result<T, E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Stream::poll_next(Pin::new(&mut self.rx), cx)
    }
}

pub struct TransferSink<T, R, E> {
    tx: Sender<Result<T, E>>,
    fut: Option<LocalBoxFuture<'static, Result<R, E>>>,
}

impl<T, R, E> TransferSink<T, R, E> {
    pub fn create(channel_size: usize) -> (Self, Receiver<Result<T, E>>) {
        let (tx, rx) = channel(channel_size);
        (TransferSink { tx, fut: None }, rx)
    }

    pub fn take_future(&mut self) -> Option<LocalBoxFuture<'static, Result<R, E>>> {
        self.fut.take()
    }
}

impl<T, R> Sink<T> for TransferSink<T, R, Error> {
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
