mod archive;
pub mod cache;
mod container;
pub mod error;
mod file;
mod gftp;
mod hash;
mod http;
mod location;
mod progress;
mod retry;
pub mod transfer;
mod traverse;

use std::cell::RefCell;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures::channel::mpsc::{channel, Receiver, Sender};
use futures::channel::oneshot;
use futures::future::{AbortHandle, AbortRegistration, Abortable, Aborted, LocalBoxFuture};
use futures::prelude::*;
use futures::task::{Context, Poll};
use url::Url;

use crate::error::Error;

pub use crate::archive::{archive, extract, ArchiveFormat};
pub use crate::container::ContainerTransferProvider;
pub use crate::file::{DirTransferProvider, FileTransferProvider};
pub use crate::gftp::GftpTransferProvider;
pub use crate::http::HttpTransferProvider;
pub use crate::location::{TransferUrl, UrlExt};
pub use crate::progress::{wrap_sink_with_progress_reporting, wrap_stream_with_progress_reporting};
pub use crate::retry::Retry;
pub use crate::traverse::PathTraverse;

use crate::hash::with_hash_stream;
use crate::progress::progress_report_channel;
use crate::transfer::Progress;
use ya_client_model::activity::TransferArgs;

/// Transfers data from `stream` to a `TransferSink`
pub async fn transfer<S, T>(stream: S, mut sink: TransferSink<T, Error>) -> Result<(), Error>
where
    S: Stream<Item = Result<T, Error>>,
{
    let rx = sink.res_rx.take().unwrap();
    stream.forward(sink).await?;
    rx.await?
}

/// Transfers data between `TransferProvider`s within current context
pub async fn transfer_with<S, D>(
    src: impl AsRef<S>,
    src_url: &TransferUrl,
    dst: impl AsRef<D>,
    dst_url: &TransferUrl,
    ctx: &TransferContext,
) -> Result<(), Error>
where
    S: TransferProvider<TransferData, Error> + ?Sized,
    D: TransferProvider<TransferData, Error> + ?Sized,
{
    let src = src.as_ref();
    let dst = dst.as_ref();

    loop {
        let fut = async {
            dst.prepare_destination(&dst_url.url, ctx).await?;
            src.prepare_source(&src_url.url, ctx).await?;

            log::debug!("Transferring from offset: {}", ctx.state.offset());

            let stream = with_hash_stream(src.source(&src_url.url, ctx), src_url, dst_url, ctx)?;
            let sink = progress_report_channel(dst.destination(&dst_url.url, ctx), ctx);

            transfer(stream, sink).await?;
            Ok::<_, Error>(())
        };

        match fut.await {
            Ok(val) => return Ok(val),
            Err(err) => match ctx.state.delay(&err) {
                Some(delay) => {
                    log::warn!("Retrying in {}s because: {}", delay.as_secs_f32(), err);
                    tokio::time::sleep(delay).await;
                }
                None => return Err(err),
            },
        };
    }
}

/// Trait for implementing file transfer methods
pub trait TransferProvider<T, E> {
    /// Returns the URL schemes supported by this provider, e.g. `vec!["http", "https"]`
    fn schemes(&self) -> Vec<&'static str>;

    /// Creates a transfer stream from `url` within current context
    fn source(&self, url: &Url, ctx: &TransferContext) -> TransferStream<T, E>;
    /// Creates a transfer sink to `url` within current context
    fn destination(&self, url: &Url, ctx: &TransferContext) -> TransferSink<T, E>;

    /// Initializes the transfer context when acting as a stream.
    /// Executed prior to `source`, but after `prepare_destination`
    fn prepare_source<'a>(
        &self,
        _url: &Url,
        ctx: &TransferContext,
    ) -> LocalBoxFuture<'a, Result<(), Error>> {
        ctx.state.set_offset(0);
        futures::future::ok(()).boxed_local()
    }

    /// Initializes the transfer context when acting as a sink.
    /// Executed prior to `destination` and `prepare_source`
    fn prepare_destination<'a>(
        &self,
        _url: &Url,
        ctx: &TransferContext,
    ) -> LocalBoxFuture<'a, Result<(), Error>> {
        ctx.state.set_offset(0);
        futures::future::ok(()).boxed_local()
    }
}

type InnerStream<'a, I> = Pin<Box<dyn Stream<Item = I> + Send + Sync + Unpin + 'a>>;

pub struct TransferStream<T, E> {
    rx: Option<InnerStream<'static, Result<T, E>>>,
    abort_handle: AbortHandle,
}

impl<T, E> TransferStream<T, E>
where
    T: Send + 'static,
    E: Send + 'static,
{
    pub fn create(channel_size: usize) -> (Self, Sender<Result<T, E>>, AbortRegistration) {
        let (tx, rx) = channel(channel_size);
        let rx: Option<InnerStream<Result<T, E>>> = Some(Box::pin(rx));
        let (abort_handle, abort_reg) = AbortHandle::new_pair();
        (TransferStream { rx, abort_handle }, tx, abort_reg)
    }

    pub fn map_inner<F>(&mut self, f: F)
    where
        F: FnMut(Result<T, E>) -> Result<T, E> + Send + Sync + 'static,
    {
        // This function cannot take `self` as argument since `Self` implements `Drop`.
        // In order to take and map the stream, then put it back in `self.rx`, the simplest
        // workaround is to use `Option`. Outside of this function, `self.rx` is guaranteed
        // to always be `Some`
        let rx = self.rx.take().unwrap();
        self.rx.replace(Box::pin(rx.map(f)));
    }

    pub fn map_inner_async<F, Fut>(&mut self, f: F)
    where
        F: FnMut(Result<T, E>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T, E>> + Send + Sync + Unpin + 'static,
    {
        // This function cannot take `self` as argument since `Self` implements `Drop`.
        // In order to take and map the stream, then put it back in `self.rx`, the simplest
        // workaround is to use `Option`. Outside of this function, `self.rx` is guaranteed
        // to always be `Some`
        let rx = self.rx.take().unwrap();
        self.rx.replace(Box::pin(rx.then(f)));
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
        let rx = self.rx.as_mut().unwrap();
        Stream::poll_next(Pin::new(rx), cx)
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

#[allow(clippy::type_complexity)]
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

impl From<Box<[u8]>> for TransferData {
    fn from(b: Box<[u8]>) -> Self {
        Self::from(b.into_vec())
    }
}

/// Transfer context, holding information on current state
/// and arguments provided by the Requestor
#[derive(Default, Clone)]
pub struct TransferContext {
    pub state: TransferState,
    pub args: TransferArgs,
    pub report: Arc<std::sync::Mutex<Option<tokio::sync::watch::Sender<Progress>>>>,
}

impl TransferContext {
    pub fn new(offset: u64) -> Self {
        let args = TransferArgs::default();
        let state = TransferState::default();
        state.set_offset(offset);

        Self {
            args,
            state,
            report: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn register_reporter(&self, report: tokio::sync::watch::Sender<Progress>) {
        *self.report.lock().unwrap() = Some(report);
    }

    pub fn take_reporter(&self) -> Option<tokio::sync::watch::Sender<Progress>> {
        self.report.lock().unwrap().take()
    }
}

impl From<TransferArgs> for TransferContext {
    fn from(args: TransferArgs) -> Self {
        Self {
            args,
            ..Default::default()
        }
    }
}

#[derive(Clone, Default)]
pub struct TransferState {
    inner: Rc<RefCell<TransferStateInner>>,
}

impl TransferState {
    pub fn finished(&self) -> bool {
        if let Some(size) = self.size() {
            return self.offset() >= size;
        }
        false
    }

    pub fn offset(&self) -> u64 {
        self.inner.borrow().offset
    }

    pub fn set_offset(&self, offset: u64) {
        let mut r = self.inner.borrow_mut();
        r.offset = offset;
    }

    pub fn size(&self) -> Option<u64> {
        self.inner.borrow().size
    }

    pub fn set_size(&self, size: Option<u64>) {
        let mut r = self.inner.borrow_mut();
        r.size = r.size.max(size);
    }

    pub fn retry(&self, count: i32) {
        self.retry_with(Retry::new(count));
    }

    pub fn retry_with(&self, retry: Retry) {
        let mut r = self.inner.borrow_mut();
        r.retry.replace(retry);
    }

    pub fn delay(&self, err: &Error) -> Option<Duration> {
        self.inner
            .borrow_mut()
            .retry
            .as_mut()
            .and_then(|r| r.delay(err))
    }
}

struct TransferStateInner {
    offset: u64,
    size: Option<u64>,
    retry: Option<Retry>,
}

impl Default for TransferStateInner {
    fn default() -> Self {
        Self {
            offset: Default::default(),
            size: Default::default(),
            retry: Some(Retry::default()),
        }
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
            if let Err(e) = match r {
                Ok(r) => r,
                Err(e) => Err(e),
            } {
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
