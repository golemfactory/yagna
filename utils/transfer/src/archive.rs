use crate::error::Error;
use crate::TransferData;
use async_compression::stream::{BzDecoder, BzEncoder};
use async_compression::stream::{GzipDecoder, GzipEncoder};
use async_compression::stream::{XzDecoder, XzEncoder};
use bytes::Bytes;
use futures::channel::{mpsc, mpsc::Sender};
use futures::task::{Context, Poll};
use futures::{FutureExt, Sink, SinkExt, Stream, StreamExt, TryStreamExt};
use rand::Rng;
use std::convert::TryFrom;
use std::fs::create_dir_all;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::str::FromStr;
use tokio::fs::OpenOptions;
use tokio::io::{copy, AsyncWriteExt};
use ya_client_model::activity::TransferArgs;
use ya_utils_path::normalize_path;
use zip::tokio::read::read_zipfile_from_stream;
use zip::write::FileOptions;

#[derive(Clone, Copy, Debug)]
pub enum ArchiveFormat {
    Tar,
    TarBz2,
    TarGz,
    TarXz,
    Zip,
    ZipStored,
}

impl Default for ArchiveFormat {
    fn default() -> Self {
        ArchiveFormat::TarGz
    }
}

impl FromStr for ArchiveFormat {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.to_lowercase();
        if s.ends_with(".tar") {
            Ok(ArchiveFormat::Tar)
        } else if s.ends_with(".tar.bz2") {
            Ok(ArchiveFormat::TarBz2)
        } else if s.ends_with(".tar.gz") {
            Ok(ArchiveFormat::TarGz)
        } else if s.ends_with(".tar.xz") {
            Ok(ArchiveFormat::TarXz)
        } else if s.ends_with(".zip") {
            Ok(ArchiveFormat::Zip)
        } else if s.ends_with(".zip.0") {
            Ok(ArchiveFormat::ZipStored)
        } else {
            Err(Error::InvalidUrlError("Unsupported format".into()))
        }
    }
}

impl<'s> TryFrom<&'s str> for ArchiveFormat {
    type Error = Error;

    fn try_from(s: &'s str) -> Result<Self, Self::Error> {
        let format = match s.to_lowercase().as_str() {
            "tar" => ArchiveFormat::Tar,
            "tar.bz2" => ArchiveFormat::TarBz2,
            "tar.gz" => ArchiveFormat::TarGz,
            "tar.xz" => ArchiveFormat::TarXz,
            "zip" => ArchiveFormat::Zip,
            "zip.0" => ArchiveFormat::ZipStored,
            _ => return Err(Error::OutputFormat(s.to_string())),
        };
        Ok(format)
    }
}

impl<'s> TryFrom<&TransferArgs> for ArchiveFormat {
    type Error = Error;

    fn try_from(args: &TransferArgs) -> Result<Self, Self::Error> {
        match &args.format {
            Some(format) => ArchiveFormat::try_from(format.as_str()),
            None => Err(Error::OutputFormat("No archive format specified".into())),
        }
    }
}

#[derive(Clone, Debug)]
pub enum FileEvent {
    Processing {
        name: PathBuf,
        size: usize,
        is_dir: bool,
    },
    Finished {
        name: PathBuf,
    },
}

#[inline]
pub async fn archive<'a, P, R>(
    path_iter: impl Iterator<Item = R> + 'static,
    path_root: P,
    format: ArchiveFormat,
    evt_sender: Sender<FileEvent>,
) -> Pin<Box<dyn Stream<Item = Result<TransferData, Error>> + Send + Sync + 'a>>
where
    P: AsRef<Path> + Send + Sync + 'a,
    R: AsRef<Path> + Unpin + Send + Sync + 'static,
{
    archive_stream(path_iter, path_root, format, evt_sender).await
}

pub async fn archive_stream<'a, B, E, P, R>(
    path_iter: impl Iterator<Item = R> + 'static,
    path_root: P,
    format: ArchiveFormat,
    evt_sender: Sender<FileEvent>,
) -> Pin<Box<dyn Stream<Item = Result<B, E>> + Send + Sync + 'a>>
where
    B: From<Bytes> + Unpin + Send + Sync + 'a,
    E: From<io::Error> + Unpin + Send + Sync + 'a,
    P: AsRef<Path> + Send + Sync + 'a,
    R: AsRef<Path> + Unpin + Send + Sync + 'static,
{
    let path_root = match normalize_path(path_root.as_ref()) {
        Ok(path) => path,
        Err(error) => return Box::pin(futures_stream_err(error).map(BytesResult::convert)),
    };

    match format {
        ArchiveFormat::Tar => Box::pin(
            archive_tar(path_iter, path_root, evt_sender)
                .await
                .map(BytesResult::convert),
        ),
        ArchiveFormat::TarBz2 => Box::pin(
            BzEncoder::new(archive_tar(path_iter, path_root, evt_sender).await)
                .map(BytesResult::convert),
        ),
        ArchiveFormat::TarGz => Box::pin(
            GzipEncoder::new(archive_tar(path_iter, path_root, evt_sender).await)
                .map(BytesResult::convert),
        ),
        ArchiveFormat::TarXz => Box::pin(
            XzEncoder::new(archive_tar(path_iter, path_root, evt_sender).await)
                .map(BytesResult::convert),
        ),
        ArchiveFormat::Zip | ArchiveFormat::ZipStored => {
            archive_zip(
                path_iter,
                path_root,
                match format {
                    ArchiveFormat::ZipStored => zip::CompressionMethod::Stored,
                    _ => zip::CompressionMethod::Deflated,
                },
                evt_sender,
            )
            .await
        }
    }
}

async fn archive_tar<'a, P, R>(
    path_iter: impl Iterator<Item = R> + 'static,
    path_root: P,
    mut evt_sender: Sender<FileEvent>,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Unpin + Send + Sync + 'a
where
    P: AsRef<Path> + 'a,
    R: AsRef<Path> + 'a,
{
    let path_root = path_root.as_ref().to_owned();
    let (tx, rx) = mpsc::channel::<Result<Bytes, io::Error>>(1);
    let mut txe = tx.clone();

    let fut = async move {
        let mut path_iter = path_iter.peekable();
        if let None = path_iter.peek() {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }

        let writer = TokioAsyncWrite(
            tx.sink_map_err(|e| io_error(e))
                .with(|b| futures::future::ok::<_, io::Error>(Ok(b))),
        );
        let mut builder = tokio_tar::Builder::new(writer);

        for prov in path_iter {
            let path = prov.as_ref();
            let metadata = std::fs::metadata(&path)?;
            let name = path.strip_prefix(&path_root).map_err(io_error)?;

            let _ = evt_sender
                .send(FileEvent::Processing {
                    name: name.to_path_buf(),
                    size: metadata.len() as usize,
                    is_dir: metadata.is_dir(),
                })
                .await;

            builder.append_path_with_name(&path, &name).await?;

            let _ = evt_sender
                .send(FileEvent::Finished {
                    name: name.to_path_buf(),
                })
                .await;
        }

        builder.finish().await?;
        Ok(())
    }
    .then(|result| async move {
        if let Err(e) = result {
            let _ = txe.send(Err(e)).await;
        }
    });

    tokio::task::spawn_local(fut);
    rx
}

async fn archive_zip<'a, B, E, P, R>(
    path_iter: impl Iterator<Item = R> + 'static,
    path_root: P,
    method: zip::CompressionMethod,
    mut evt_sender: Sender<FileEvent>,
) -> Pin<Box<dyn Stream<Item = Result<B, E>> + Send + Sync + 'a>>
where
    B: From<Bytes> + Unpin + Send + Sync + 'a,
    E: From<io::Error> + Unpin + Send + Sync + 'a,
    P: AsRef<Path> + Send + Sync + 'a,
    R: AsRef<Path> + Unpin + Send + Sync + 'static,
{
    let path_root = path_root.as_ref().to_owned();
    let path_items = path_iter.collect::<Vec<_>>();

    let tmp_dir = match tempdir::TempDir::new("transfer-zip") {
        Ok(d) => d,
        Err(e) => return Box::pin(futures_stream_err(e).map(BytesResult::convert)),
    };
    let output = tmp_dir.path().join(format!(
        "{}.zip",
        hex::encode(rand::thread_rng().gen::<[u8; 16]>())
    ));

    let (mut tx, mut rx) = mpsc::channel::<Result<(), io::Error>>(1);
    let output_f = output.clone();

    let fut = async move {
        if path_items.is_empty() {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }

        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(output_f)?;
        let options = FileOptions::default()
            .compression_method(method)
            .unix_permissions(0o755);

        let mut zip = zip::ZipWriter::new(file);
        for prov in path_items {
            let path = prov.as_ref();
            let metadata = std::fs::metadata(&path)?;
            let name = path.strip_prefix(&path_root).map_err(io_error)?;

            let _ = evt_sender
                .send(FileEvent::Processing {
                    name: name.to_path_buf(),
                    size: metadata.len() as usize,
                    is_dir: metadata.is_dir(),
                })
                .await;

            if path.is_dir() {
                zip.add_directory_from_path(name, options)
                    .map_err(io_error)?;
            } else {
                let mut f = std::fs::File::open(&path)?;
                zip.start_file_from_path(name, options).map_err(io_error)?;
                std::io::copy(&mut f, &mut zip)?;
            }

            let _ = evt_sender
                .send(FileEvent::Finished {
                    name: name.to_path_buf(),
                })
                .await;
        }

        zip.finish().map_err(io_error)?;
        Ok::<_, io::Error>(())
    }
    .then(|result| async move {
        if let Err(e) = result {
            let _ = tx.send(Err(e)).await;
        }
    });

    tokio::task::spawn(fut);

    if let Some(Err(e)) = rx.next().await {
        return Box::pin(futures_stream_err(io_error(e)).map(BytesResult::convert));
    }

    match OpenOptions::new().read(true).open(output.clone()).await {
        Ok(f) => Box::pin(FuturesStream::new(f).map(BytesResult::convert)),
        Err(e) => Box::pin(futures_stream_err(e).map(BytesResult::convert)),
    }
}

pub async fn extract<'a, B, S, E, P>(
    stream: S,
    path: P,
    format: ArchiveFormat,
    evt_sender: Sender<FileEvent>,
) -> Result<(), E>
where
    B: Into<Bytes>,
    S: Stream<Item = Result<B, E>> + Unpin + Send + Sync + 'a,
    E: Into<io::Error> + From<io::Error> + 'a,
    P: AsRef<Path> + 'a,
{
    std::fs::create_dir_all(&path)?;
    let path = normalize_path(path.as_ref())?;

    let stream = stream.map(|r| r.map(|b| b.into()).map_err(|e| e.into()));
    match format {
        ArchiveFormat::Tar => {
            extract_tar(stream, path, evt_sender).await?;
        }
        ArchiveFormat::TarBz2 => {
            extract_tar(BzDecoder::new(stream).into_stream(), path, evt_sender).await?;
        }
        ArchiveFormat::TarGz => {
            extract_tar(GzipDecoder::new(stream).into_stream(), path, evt_sender).await?;
        }
        ArchiveFormat::TarXz => {
            extract_tar(XzDecoder::new(stream).into_stream(), path, evt_sender).await?;
        }
        ArchiveFormat::Zip | ArchiveFormat::ZipStored => {
            extract_zip(stream, path, evt_sender).await?;
        }
    }
    Ok(())
}

async fn extract_zip<'a, S, P>(
    stream: S,
    path: P,
    mut evt_sender: Sender<FileEvent>,
) -> Result<(), io::Error>
where
    S: Stream<Item = Result<Bytes, io::Error>> + Unpin + Send + Sync + 'a,
    P: AsRef<Path>,
{
    let path = path.as_ref();
    let mut reader = TokioAsyncRead(stream.into_async_read());

    loop {
        if let Some(mut result) = read_zipfile_from_stream(&mut reader)
            .await
            .map_err(|e| io_error(e))?
        {
            let name = result.sanitized_name();
            let size = result.size() as usize;
            let file_path = path.join(&name);

            let _ = evt_sender
                .send(FileEvent::Processing {
                    name: name.clone(),
                    size,
                    is_dir: result.is_dir(),
                })
                .await;

            if result.is_dir() {
                create_dir_all(file_path)?;
            } else {
                if let Some(parent) = file_path.parent() {
                    create_dir_all(parent)?;
                }
                let mut file = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(file_path)
                    .await?;
                copy(&mut result, &mut file).await?;
                file.flush().await?;
            }

            let _ = evt_sender.send(FileEvent::Finished { name }).await;
            result.exhaust().await;
        } else {
            break;
        }
    }

    Ok(())
}

async fn extract_tar<'a, S, P>(
    stream: S,
    path: P,
    mut evt_sender: Sender<FileEvent>,
) -> Result<(), io::Error>
where
    S: Stream<Item = Result<Bytes, io::Error>> + Unpin + Send + Sync + 'a,
    P: AsRef<Path>,
{
    let path = path.as_ref();
    let stream = TokioAsyncRead(stream.into_async_read());
    let mut archive = tokio_tar::Archive::new(stream);
    let mut entries = archive.entries()?;

    while let Some(file) = entries.next().await {
        let mut file = file?;
        let header = file.header();
        let name = file.path()?.to_path_buf();

        let evt = FileEvent::Processing {
            name: name.clone(),
            size: header.size().ok().unwrap_or(0) as usize,
            is_dir: match header.entry_type() {
                tokio_tar::EntryType::Directory => true,
                _ => false,
            },
        };
        let _ = evt_sender.send(evt).await;

        file.unpack_in(path).await?;

        let _ = evt_sender.send(FileEvent::Finished { name }).await;
    }

    Ok(())
}

#[inline(always)]
fn io_error<E>(err: E) -> io::Error
where
    E: std::error::Error + Unpin + Send + Sync + 'static,
{
    io::Error::new(io::ErrorKind::Other, err)
}

struct TokioAsyncRead<R>(R);

impl<R> tokio::io::AsyncRead for TokioAsyncRead<R>
where
    R: futures::AsyncRead + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<tokio::io::Result<usize>> {
        Pin::new(&mut self.0)
            .poll_read(cx, buf)
            .map_err(|e| io_error(e))
    }
}

struct TokioAsyncWrite<S>(S);

impl<S> tokio::io::AsyncWrite for TokioAsyncWrite<S>
where
    S: Sink<Bytes, Error = io::Error> + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let this = self.get_mut();

        match Sink::poll_ready(Pin::new(&mut this.0), cx) {
            Poll::Ready(Ok(_)) => (),
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Pending => return Poll::Pending,
        }
        match Sink::start_send(Pin::new(&mut this.0), Bytes::copy_from_slice(buf)) {
            Ok(_) => Poll::Ready(Ok(buf.len())),
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let this = self.get_mut();
        Sink::poll_flush(Pin::new(&mut this.0), cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let this = self.get_mut();
        Sink::poll_close(Pin::new(&mut this.0), cx)
    }
}

struct AsyncReadErr(io::Error);

impl tokio::io::AsyncRead for AsyncReadErr {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let error = io::Error::from(this.0.kind());
        Poll::Ready(Err(error))
    }
}

fn futures_stream_err(error: io::Error) -> FuturesStream<AsyncReadErr> {
    FuturesStream::new(AsyncReadErr(error))
}

const BUF_SIZE: usize = 4096;

struct FuturesStream<R> {
    buf: [u8; BUF_SIZE],
    reader: R,
}

impl<R> FuturesStream<R> {
    fn new(reader: R) -> Self {
        FuturesStream {
            buf: [0u8; BUF_SIZE],
            reader,
        }
    }
}

impl<R> Stream for FuturesStream<R>
where
    R: tokio::io::AsyncRead + Sized + Send + Sync + Unpin,
{
    type Item = Result<Bytes, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match Pin::new(&mut this.reader).poll_read(cx, &mut this.buf) {
            Poll::Ready(Ok(0)) => Poll::Ready(None),
            Poll::Ready(Ok(n)) => Poll::Ready(Some(Ok(Bytes::copy_from_slice(&this.buf[..n])))),
            Poll::Ready(Err(e)) => Poll::Ready(Some(Err(e))),
            Poll::Pending => Poll::Pending,
        }
    }
}

trait BytesResult {
    fn convert<B, E>(self) -> Result<B, E>
    where
        B: From<Bytes>,
        E: From<io::Error>;
}

impl BytesResult for Result<Bytes, io::Error> {
    fn convert<B, E>(self) -> Result<B, E>
    where
        B: From<Bytes>,
        E: From<io::Error>,
    {
        self.map(|b| B::from(b)).map_err(|e| E::from(e))
    }
}
