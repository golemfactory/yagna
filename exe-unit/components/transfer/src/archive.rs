use crate::error::Error;
use crate::TransferData;
use async_compression::futures::bufread::{BzDecoder, BzEncoder};
use async_compression::futures::bufread::{GzipDecoder, GzipEncoder};
use async_compression::futures::bufread::{XzDecoder, XzEncoder};
use bytes::{Bytes, BytesMut};
use futures::channel::{mpsc, mpsc::Sender};
use futures::task::{Context, Poll};
use futures::{AsyncRead, FutureExt, Sink, SinkExt, Stream, StreamExt, TryStreamExt};
use rand::Rng;
use std::convert::TryFrom;
use std::fs::create_dir_all;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::str::FromStr;
use tokio::fs::OpenOptions;
use tokio::io::{copy, AsyncWriteExt, ReadBuf};
use ya_client_model::activity::TransferArgs;
use ya_utils_path::normalize_path;
use zip::tokio::read::read_zipfile_from_stream;
use zip::write::FileOptions;

#[derive(Clone, Copy, Debug, Default)]
pub enum ArchiveFormat {
    Tar,
    TarBz2,
    #[default]
    TarGz,
    TarXz,
    Zip,
    ZipStored,
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

impl TryFrom<&TransferArgs> for ArchiveFormat {
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
    let path_root = match normalize_path(&path_root) {
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
            codec_stream(BzEncoder::new(
                archive_tar(path_iter, path_root, evt_sender)
                    .await
                    .into_async_read(),
            ))
            .map(BytesResult::convert),
        ),
        ArchiveFormat::TarGz => Box::pin(
            codec_stream(GzipEncoder::new(
                archive_tar(path_iter, path_root, evt_sender)
                    .await
                    .into_async_read(),
            ))
            .map(BytesResult::convert),
        ),
        ArchiveFormat::TarXz => Box::pin(
            codec_stream(XzEncoder::new(
                archive_tar(path_iter, path_root, evt_sender)
                    .await
                    .into_async_read(),
            ))
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
        if path_iter.peek().is_none() {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }

        let writer = TokioAsyncWrite(
            tx.sink_map_err(io_error)
                .with(|b| futures::future::ok::<_, io::Error>(Ok(b))),
        );
        let mut builder = tokio_tar::Builder::new(writer);

        for prov in path_iter {
            let path = prov.as_ref();
            let metadata = std::fs::metadata(path)?;
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
            let metadata = std::fs::metadata(path)?;
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
                let mut f = std::fs::File::open(path)?;
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

pub async fn extract<B, S, E, P>(
    stream: S,
    path: P,
    format: ArchiveFormat,
    evt_sender: Sender<FileEvent>,
) -> Result<(), E>
where
    B: Into<Bytes>,
    S: Stream<Item = Result<B, E>> + Unpin + Send + Sync + 'static,
    E: Into<io::Error> + From<io::Error> + 'static,
    P: AsRef<Path> + 'static,
{
    std::fs::create_dir_all(&path)?;
    let path = normalize_path(path.as_ref())?;

    let stream = stream.map(|r| r.map(Into::into).map_err(Into::into));
    match format {
        ArchiveFormat::Tar => {
            extract_tar(stream, path, evt_sender).await?;
        }
        ArchiveFormat::TarBz2 => {
            extract_tar(
                codec_stream(BzDecoder::new(stream.into_async_read())),
                path,
                evt_sender,
            )
            .await?;
        }
        ArchiveFormat::TarGz => {
            extract_tar(
                codec_stream(GzipDecoder::new(stream.into_async_read())),
                path,
                evt_sender,
            )
            .await?;
        }
        ArchiveFormat::TarXz => {
            extract_tar(
                codec_stream(XzDecoder::new(stream.into_async_read())),
                path,
                evt_sender,
            )
            .await?;
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

    while let Some(mut result) = read_zipfile_from_stream(&mut reader)
        .await
        .map_err(io_error)?
    {
        validate_archive_entry_path(Path::new(result.name()))?;
        if let Some(mode) = result.unix_mode() {
            let file_type = mode & 0o170000;
            if file_type != 0 && file_type != 0o100000 && file_type != 0o040000 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("unsafe zip entry type rejected: {file_type:#o}"),
                ));
            }
        }

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
    }

    Ok(())
}

async fn extract_tar<'a, S, P>(
    stream: S,
    path: P,
    mut evt_sender: Sender<FileEvent>,
) -> Result<(), io::Error>
where
    S: Stream<Item = Result<Bytes, io::Error>> + Send + Sync + Unpin + 'a,
    P: AsRef<Path>,
{
    let path = path.as_ref();
    let stream = TokioAsyncRead(stream.into_async_read());
    let mut archive = tokio_tar::Archive::new(stream);
    let mut entries = archive.entries()?;

    while let Some(file) = entries.next().await {
        let mut file = file?;
        let header = file.header();
        let entry_type = header.entry_type();
        // tokio-tar consumes GNU long names and pax local extensions itself, but yields
        // global pax headers (`git archive` emits one) as ordinary entries. They carry
        // metadata only and unpack to nothing.
        if entry_type.is_pax_global_extensions() || entry_type.is_pax_local_extensions() {
            continue;
        }
        if !entry_type.is_file() && !entry_type.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("unsafe tar entry type rejected: {entry_type:?}"),
            ));
        }

        let name = file.path()?.to_path_buf();
        validate_archive_entry_path(&name)?;

        let evt = FileEvent::Processing {
            name: name.clone(),
            size: header.size().ok().unwrap_or(0) as usize,
            is_dir: entry_type == tokio_tar::EntryType::Directory,
        };
        let _ = evt_sender.send(evt).await;

        if file.unpack_in(path).await?.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("unsafe tar entry path rejected: {}", name.display()),
            ));
        }

        let _ = evt_sender.send(FileEvent::Finished { name }).await;
    }

    Ok(())
}

fn validate_archive_entry_path(path: &Path) -> io::Result<()> {
    let path_str = path
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 archive path"))?;
    if path_str.contains('\\') || path_str.contains('\0') || path_str.contains(':') {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("unsafe archive path rejected: {}", path.display()),
        ));
    }

    let mut has_component = false;
    for component in path.components() {
        match component {
            std::path::Component::Normal(_) => has_component = true,
            // `tar -cf out.tar .` prefixes every entry with `./`. A `.` contributes
            // nothing to the resolved path, and `unpack_in` skips such components.
            std::path::Component::CurDir => has_component = true,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("unsafe archive path rejected: {}", path.display()),
                ))
            }
        }
    }
    if !has_component {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "empty archive path rejected",
        ));
    }
    Ok(())
}

fn codec_stream<R>(s: R) -> Pin<Box<dyn Stream<Item = io::Result<Bytes>> + Send + Sync + 'static>>
where
    R: AsyncRead + Send + Sync + Unpin + 'static,
{
    use futures::io::AsyncReadExt;

    let stream = futures::stream::try_unfold(s, |mut encoder| async move {
        let mut chunk = BytesMut::with_capacity(40 * 1024);
        chunk.resize(40 * 1024, 0);
        match encoder.read(&mut chunk).await? {
            0 => Ok(None),
            len => {
                chunk.truncate(len);
                Ok(Some((chunk.freeze(), encoder)))
            }
        }
    });
    Box::pin(stream)
}

#[inline(always)]
fn io_error<E>(err: E) -> io::Error
where
    E: std::error::Error + Unpin + Send + Sync + 'static,
{
    io::Error::other(err)
}

struct TokioAsyncRead<R>(R);

impl<R> tokio::io::AsyncRead for TokioAsyncRead<R>
where
    R: futures::AsyncRead + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let unfilled = buf.initialize_unfilled();
        match Pin::new(&mut self.0).poll_read(cx, unfilled) {
            Poll::Ready(Ok(read)) => {
                buf.advance(read);
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(io_error(e))),
            Poll::Pending => Poll::Pending,
        }
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
        _: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
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
        let mut read = ReadBuf::new(&mut this.buf);
        match Pin::new(&mut this.reader).poll_read(cx, &mut read) {
            Poll::Ready(Ok(())) => match read.filled().len() {
                0 => Poll::Ready(None),
                n => Poll::Ready(Some(Ok(Bytes::copy_from_slice(&read.filled()[..n])))),
            },
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

#[cfg(test)]
mod tests {
    use super::*;
    use futures::TryStreamExt;

    const TAR_BLOCK_SIZE: usize = 512;

    fn write_tar_octal(field: &mut [u8], value: u64) {
        field.fill(b'0');
        let value = format!("{value:o}");
        let offset = field.len() - value.len() - 1;
        field[offset..offset + value.len()].copy_from_slice(value.as_bytes());
        field[field.len() - 1] = 0;
    }

    fn tar_header(name: &str, size: u64, entry_type: u8) -> [u8; TAR_BLOCK_SIZE] {
        let mut header = [0u8; TAR_BLOCK_SIZE];
        header[..name.len()].copy_from_slice(name.as_bytes());
        write_tar_octal(&mut header[100..108], 0o644);
        write_tar_octal(&mut header[108..116], 0);
        write_tar_octal(&mut header[116..124], 0);
        write_tar_octal(&mut header[124..136], size);
        write_tar_octal(&mut header[136..148], 0);
        header[148..156].fill(b' ');
        header[156] = entry_type;
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");

        let checksum: u64 = header.iter().map(|byte| *byte as u64).sum();
        header[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
        header
    }

    fn pax_record(key: &str, value: &str) -> Vec<u8> {
        let body = format!("{key}={value}\n");
        let mut length = body.len() + 2;
        loop {
            let record = format!("{length} {body}");
            if record.len() == length {
                return record.into_bytes();
            }
            length = record.len();
        }
    }

    #[actix_rt::test]
    async fn tar_gz_archive_emits_bytes() -> anyhow::Result<()> {
        let temp = tempdir::TempDir::new("archive-stream-test")?;
        let file = temp.path().join("file.txt");
        std::fs::write(&file, b"archive payload")?;
        let file = file.canonicalize()?;
        let (events, mut event_rx) = mpsc::channel(1);
        tokio::task::spawn_local(async move { while event_rx.next().await.is_some() {} });

        let chunks = archive(
            vec![file].into_iter(),
            temp.path().to_path_buf(),
            ArchiveFormat::TarGz,
            events,
        )
        .await
        .try_collect::<Vec<_>>()
        .await?;

        assert!(chunks.iter().any(|chunk| !chunk.as_ref().is_empty()));
        Ok(())
    }

    #[actix_rt::test]
    async fn zip_archive_round_trips() -> anyhow::Result<()> {
        let source = tempdir::TempDir::new("archive-zip-source-test")?;
        let file = source.path().join("file.txt");
        std::fs::write(&file, b"zip archive payload")?;
        let file = file.canonicalize()?;
        let (archive_events, mut archive_event_rx) = mpsc::channel(2);
        tokio::task::spawn_local(async move { while archive_event_rx.next().await.is_some() {} });

        let chunks = archive(
            vec![file].into_iter(),
            source.path().to_path_buf(),
            ArchiveFormat::Zip,
            archive_events,
        )
        .await
        .try_collect::<Vec<_>>()
        .await?;

        let destination = tempdir::TempDir::new("archive-zip-destination-test")?;
        let stream = futures::stream::iter(chunks.into_iter().map(Ok::<_, io::Error>));
        let (extract_events, mut extract_event_rx) = mpsc::channel(2);
        tokio::task::spawn_local(async move { while extract_event_rx.next().await.is_some() {} });
        extract(
            stream,
            destination.path().to_path_buf(),
            ArchiveFormat::Zip,
            extract_events,
        )
        .await?;

        assert_eq!(
            std::fs::read(destination.path().join("file.txt"))?,
            b"zip archive payload"
        );
        Ok(())
    }

    #[actix_rt::test]
    async fn tar_pax_size_does_not_create_smuggled_entry() -> anyhow::Result<()> {
        let pax = pax_record("size", TAR_BLOCK_SIZE.to_string().as_str());
        let mut archive = Vec::new();
        archive.extend_from_slice(&tar_header("pax-header", pax.len() as u64, b'x'));
        archive.extend_from_slice(&pax);
        archive.resize(archive.len().next_multiple_of(TAR_BLOCK_SIZE), 0);
        archive.extend_from_slice(&tar_header("safe.txt", 0, b'0'));
        archive.extend_from_slice(&tar_header("smuggled.txt", 0, b'0'));
        archive.extend_from_slice(&[0; TAR_BLOCK_SIZE * 2]);

        let temp = tempdir::TempDir::new("archive-pax-smuggling-test")?;
        let stream = futures::stream::iter(vec![Ok(Bytes::from(archive))]);
        let (events, mut event_rx) = mpsc::channel(4);
        tokio::task::spawn_local(async move { while event_rx.next().await.is_some() {} });

        extract_tar(stream, temp.path(), events).await?;

        assert_eq!(
            std::fs::metadata(temp.path().join("safe.txt"))?.len(),
            TAR_BLOCK_SIZE as u64
        );
        assert!(!temp.path().join("smuggled.txt").exists());
        Ok(())
    }

    #[test]
    fn archive_entry_path_validation_rejects_unsafe_paths() {
        for path in [
            "../escaped",
            "safe/../../escaped",
            "/absolute",
            "back\\slash",
            "file:stream",
        ] {
            assert!(
                validate_archive_entry_path(Path::new(path)).is_err(),
                "{} should be rejected",
                path
            );
        }
        assert!(validate_archive_entry_path(Path::new("safe/nested/file")).is_ok());
        assert!(validate_archive_entry_path(Path::new("./")).is_ok());
        assert!(validate_archive_entry_path(Path::new("./safe/nested/file")).is_ok());
    }

    /// `tar -cf out.tar .` names every entry `./…` and `git archive` prepends a global
    /// pax header. Neither can escape the destination, so both must extract cleanly.
    #[actix_rt::test]
    async fn extracts_tar_with_dot_slash_and_global_pax_header() -> anyhow::Result<()> {
        let mut builder = tokio_tar::Builder::new(Vec::new());

        let payload = b"pax global header payload";
        let mut header = tokio_tar::Header::new_ustar();
        header.set_entry_type(tokio_tar::EntryType::XGlobalHeader);
        header.set_size(payload.len() as u64);
        header.set_cksum();
        builder
            .append_data(&mut header, "pax_global_header", &payload[..])
            .await?;

        let mut header = tokio_tar::Header::new_gnu();
        header.set_entry_type(tokio_tar::EntryType::Directory);
        header.set_size(0);
        header.set_mode(0o755);
        header.set_cksum();
        builder.append_data(&mut header, "./", &[][..]).await?;

        let contents = b"hello";
        let mut header = tokio_tar::Header::new_gnu();
        header.set_entry_type(tokio_tar::EntryType::Regular);
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "./nested/hello.txt", &contents[..])
            .await?;

        let tarball = builder.into_inner().await?;
        let stream = futures::stream::iter(vec![Ok::<Bytes, io::Error>(Bytes::from(tarball))]);
        let out = tempdir::TempDir::new("tar-compat-test")?;
        let (evt_tx, mut evt_rx) = mpsc::channel(1);
        tokio::task::spawn_local(async move { while evt_rx.next().await.is_some() {} });

        extract::<_, _, io::Error, _>(stream, out.path().to_path_buf(), ArchiveFormat::Tar, evt_tx)
            .await?;

        assert_eq!(
            std::fs::read(out.path().join("nested/hello.txt"))?,
            contents
        );
        Ok(())
    }
}
