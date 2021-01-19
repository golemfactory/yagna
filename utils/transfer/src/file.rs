use crate::archive::ArchiveFormat;
use crate::archive::{archive, extract};
use crate::error::Error;
use crate::traverse::PathTraverse;
use crate::{abortable_sink, abortable_stream};
use crate::{TransferData, TransferProvider, TransferSink, TransferStream};
use bytes::BytesMut;
use futures::future::ready;
use futures::{SinkExt, StreamExt, TryFutureExt, TryStreamExt};
use std::convert::TryFrom;
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::task::spawn_local;
use tokio_util::codec::{BytesCodec, FramedRead};
use url::Url;
use ya_client_model::activity::TransferArgs;

pub struct FileTransferProvider;
pub struct DirTransferProvider;

impl Default for FileTransferProvider {
    fn default() -> Self {
        FileTransferProvider {}
    }
}

impl TransferProvider<TransferData, Error> for FileTransferProvider {
    fn schemes(&self) -> Vec<&'static str> {
        vec!["file"]
    }

    fn source(&self, url: &Url, _: &TransferArgs) -> TransferStream<TransferData, Error> {
        let (stream, tx, abort_reg) = TransferStream::<TransferData, Error>::create(1);
        let txc = tx.clone();
        let url = url.clone();

        tokio::task::spawn_local(async move {
            let fut = async move {
                let file = File::open(extract_file_url(&url)).await?;
                FramedRead::new(file, BytesCodec::new())
                    .map_ok(BytesMut::freeze)
                    .map_err(Error::from)
                    .into_stream()
                    .forward(
                        tx.sink_map_err(Error::from)
                            .with(|b| ready(Ok(Ok(TransferData::from(b))))),
                    )
                    .await
                    .map_err(Error::from)
            };

            abortable_stream(fut, abort_reg, txc).await
        });

        stream
    }

    fn destination(&self, url: &Url, _: &TransferArgs) -> TransferSink<TransferData, Error> {
        let (sink, mut rx, res_tx) = TransferSink::<TransferData, Error>::create(1);
        let path = PathBuf::from(extract_file_url(&url));
        let path_c = path.clone();
        tokio::task::spawn_local(async move {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            log::debug!("Transfer destination file: {}", path.display());
            let fut = async move {
                let mut file = File::create(&path).await?;
                while let Some(result) = rx.next().await {
                    file.write_all(result?.as_ref()).await?;
                }
                file.flush().await?;
                file.sync_all().await?;

                Ok::<(), Error>(())
            }
            .map_err(|error| {
                log::error!(
                    "Error opening destination file [{}]: {}",
                    path_c.display(),
                    error
                );
                Error::from(error)
            });

            abortable_sink(fut, res_tx).await
        });

        sink
    }
}

impl Default for DirTransferProvider {
    fn default() -> Self {
        DirTransferProvider {}
    }
}

impl TransferProvider<TransferData, Error> for DirTransferProvider {
    fn schemes(&self) -> Vec<&'static str> {
        vec!["file"]
    }

    fn source(&self, url: &Url, args: &TransferArgs) -> TransferStream<TransferData, Error> {
        let dir = Path::new(&extract_file_url(url)).to_owned();
        let args = args.clone();
        log::debug!("Transfer source directory: {}", dir.display());

        let (stream, tx, abort_reg) = TransferStream::<TransferData, Error>::create(1);
        let txc = tx.clone();

        spawn_local(async move {
            let fut = async move {
                let format = ArchiveFormat::try_from(&args)?;
                let path_iter = args.traverse(&dir)?;

                let (evt_tx, mut evt_rx) = futures::channel::mpsc::channel(1);
                spawn_local(async move {
                    while let Some(evt) = evt_rx.next().await {
                        log::debug!("Compress: {:?}", evt);
                    }
                });

                archive(path_iter, dir, format, evt_tx)
                    .await
                    .forward(
                        tx.sink_map_err(Error::from)
                            .with(|b| ready(Ok(Ok(TransferData::from(b))))),
                    )
                    .await
            };

            abortable_stream(fut, abort_reg, txc).await
        });

        stream
    }

    fn destination(&self, url: &Url, args: &TransferArgs) -> TransferSink<TransferData, Error> {
        let dir = Path::new(&extract_file_url(url)).to_owned();
        let args = args.clone();
        log::debug!("Transfer destination directory: {}", dir.display());

        let (sink, rx, res_tx) = TransferSink::<TransferData, Error>::create(1);

        spawn_local(async move {
            let fut = async move {
                let format = ArchiveFormat::try_from(&args)?;

                let (evt_tx, mut evt_rx) = futures::channel::mpsc::channel(1);
                spawn_local(async move {
                    while let Some(evt) = evt_rx.next().await {
                        log::debug!("Extract: {:?}", evt);
                    }
                });

                extract(rx, dir, format, evt_tx).await?;
                Ok::<(), Error>(())
            };

            abortable_sink(fut, res_tx).await
        });

        sink
    }
}

pub(crate) fn extract_file_url(url: &Url) -> String {
    // On Windows, Rust implementation of Url::parse() adds a third '/' after the 'file://' indicator,
    // thus making .path() method unusable for the purposes of file creation (because File::create() will not accept that),
    // and therefore - Url hardly usable for carrying absolute file paths...
    #[cfg(windows)]
    {
        url.as_str().to_owned().replace("file:///", "")
    }
    #[cfg(not(windows))]
    {
        use crate::util::UrlExt;
        url.path_decoded()
    }
}
