mod archive;
pub mod error;
mod file;
mod gftp;
mod http;
mod retry;
mod traverse;
mod util;

use crate::error::Error;
use bytes::Bytes;
use futures::channel::mpsc::{channel, Receiver, Sender};
use futures::channel::oneshot;
use futures::future::{AbortHandle, AbortRegistration, Abortable, Aborted};
use futures::task::{Context, Poll};
use futures::{Future, FutureExt, Sink, SinkExt, Stream, StreamExt, TryFutureExt};
use sha3::digest::DynDigest;
use sha3::{Sha3_224, Sha3_256, Sha3_384, Sha3_512};
use std::pin::Pin;
use url::Url;
use ya_client_model::activity::TransferArgs;

pub use crate::archive::{archive, extract, ArchiveFormat};
pub use crate::file::{DirTransferProvider, FileTransferProvider};
pub use crate::gftp::GftpTransferProvider;
pub use crate::http::HttpTransferProvider;
pub use crate::retry::Retry;
pub use crate::traverse::PathTraverse;
pub use crate::util::UrlExt;

pub async fn transfer<S, T>(stream: S, mut sink: TransferSink<T, Error>) -> Result<(), Error>
where
    S: Stream<Item = Result<T, Error>>,
{
    let rx = sink.res_rx.take().unwrap();
    stream.forward(sink).await?;
    Ok(rx.await??)
}

pub async fn retry_transfer<S, T, Fs, Fd>(
    stream_fn: Fs,
    sink_fn: Fd,
    mut retry: Retry,
) -> Result<(), Error>
where
    S: Stream<Item = Result<T, Error>>,
    Fs: Fn() -> Result<S, Error>,
    Fd: Fn() -> TransferSink<T, Error>,
{
    loop {
        match transfer(stream_fn()?, sink_fn()).await {
            Ok(val) => return Ok(val),
            Err(err) => match retry.delay(&err) {
                Some(delay) => {
                    log::warn!("retrying in {}s: {}", delay.as_secs_f32(), err);
                    tokio::time::sleep(delay).await;
                }
                None => return Err(err),
            },
        };
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum TransferData {
    Bytes(Bytes),
}

impl AsRef<Bytes> for TransferData {
    fn as_ref(&self) -> &Bytes {
        match &self {
            TransferData::Bytes(b) => b,
        }
    }
}

impl From<TransferData> for Bytes {
    fn from(d: TransferData) -> Self {
        match d {
            TransferData::Bytes(b) => b,
        }
    }
}

impl From<Bytes> for TransferData {
    fn from(b: Bytes) -> Self {
        TransferData::Bytes(b)
    }
}

impl From<Vec<u8>> for TransferData {
    fn from(vec: Vec<u8>) -> Self {
        TransferData::Bytes(Bytes::from(vec))
    }
}

pub trait TransferProvider<T, E> {
    fn schemes(&self) -> Vec<&'static str>;

    fn source(&self, url: &Url, ctx: &TransferArgs) -> TransferStream<T, E>;
    fn destination(&self, url: &Url, ctx: &TransferArgs) -> TransferSink<T, E>;
}

pub struct TransferStream<T, E> {
    rx: Receiver<Result<T, E>>,
    abort_handle: AbortHandle,
}

impl<T: 'static, E: 'static> TransferStream<T, E> {
    pub fn create(channel_size: usize) -> (Self, Sender<Result<T, E>>, AbortRegistration) {
        let (tx, rx) = channel(channel_size);
        let (abort_handle, abort_reg) = AbortHandle::new_pair();
        (TransferStream { rx, abort_handle }, tx, abort_reg)
    }

    pub fn err(e: E) -> Self {
        let (this, mut sender, _) = Self::create(1);
        tokio::task::spawn_local(async move {
            if let Err(e) = sender.send(Err(e)).await {
                log::warn!("send error: {}", e);
            }
        });
        this
    }
}

impl<T, E> Stream for TransferStream<T, E> {
    type Item = Result<T, E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Stream::poll_next(Pin::new(&mut self.rx), cx)
    }
}

impl<T, E> Drop for TransferStream<T, E> {
    fn drop(&mut self) {
        self.abort_handle.abort();
    }
}

pub struct TransferSink<T, E> {
    tx: Sender<Result<T, E>>,
    res_rx: Option<oneshot::Receiver<Result<(), E>>>,
}

impl<T, E> TransferSink<T, E> {
    pub fn create(
        channel_size: usize,
    ) -> (Self, Receiver<Result<T, E>>, oneshot::Sender<Result<(), E>>) {
        let (tx, rx) = channel(channel_size);
        let (res_tx, res_rx) = oneshot::channel();
        (
            TransferSink {
                tx,
                res_rx: Some(res_rx),
            },
            rx,
            res_tx,
        )
    }

    pub fn err(e: E) -> Self {
        let (this, _, s) = Self::create(1);
        let _ = s.send(Err(e));
        this
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

impl<T, E> Drop for TransferSink<T, E> {
    fn drop(&mut self) {
        self.tx.close_channel();
    }
}

pub struct HashStream<T, E, S>
where
    S: Stream<Item = Result<T, E>>,
{
    inner: S,
    hasher: Box<dyn DynDigest>,
    hash: Vec<u8>,
    result: Option<Vec<u8>>,
}

impl<T, S> HashStream<T, Error, S>
where
    S: Stream<Item = Result<T, Error>> + Unpin,
{
    pub fn try_new(stream: S, alg: &str, hash: Vec<u8>) -> Result<Self, Error> {
        let hasher: Box<dyn DynDigest> = match alg {
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
                        self.hasher.input(data.as_ref());
                    }
                }
                None => {
                    let result = match &self.result {
                        Some(r) => r,
                        None => {
                            self.result = Some(self.hasher.result_reset().to_vec());
                            self.result.as_ref().unwrap()
                        }
                    };

                    if &self.hash == result {
                        log::info!("Hash verified successfully: {:?}", hex::encode(result));
                    } else {
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

fn abortable_stream<'f, T, E, F>(
    fut: F,
    abort_reg: AbortRegistration,
    mut tx: Sender<Result<T, E>>,
) -> Pin<Box<dyn Future<Output = Result<(), E>> + 'f>>
where
    F: Future<Output = Result<(), E>> + 'f,
    T: 'f,
    E: From<Aborted> + 'f,
{
    Abortable::new(fut, abort_reg)
        .map_err(E::from)
        .then(|r: Result<Result<(), E>, E>| async move {
            if let Err(e) = flatten_result(r) {
                let _ = tx.send(Err(e)).await;
            }
            tx.close_channel();
            Result::<(), E>::Ok(())
        })
        .boxed_local()
}

fn abortable_sink<'f, E, F>(
    fut: F,
    res_tx: oneshot::Sender<Result<(), E>>,
) -> Pin<Box<dyn Future<Output = Result<(), E>> + 'f>>
where
    F: Future<Output = Result<(), E>> + 'f,
    E: From<Aborted> + 'f,
{
    fut.then(|r: Result<(), E>| async move {
        let _ = match r {
            Err(e) => res_tx.send(Err(e)),
            _ => res_tx.send(Ok(())),
        };

        Result::<(), E>::Ok(())
    })
    .boxed_local()
}

#[inline(always)]
pub(crate) fn flatten_result<T, E>(r: Result<Result<T, E>, E>) -> Result<T, E> {
    r?
}
