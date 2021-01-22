use crate::archive::ArchiveFormat;
use crate::archive::{archive, extract};
use crate::error::Error;
use crate::traverse::PathTraverse;
use crate::{abortable_sink, abortable_stream};
use crate::{TransferData, TransferProvider, TransferSink, TransferStream};
use futures::future::ready;
use futures::{SinkExt, StreamExt, TryFutureExt};
use std::convert::TryFrom;
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::task::spawn_local;
use url::Url;
use ya_client_model::activity::TransferArgs;

pub struct FileTransferProvider;
pub struct DirTransferProvider;

pub const DEFAULT_CHUNK_SIZE: usize = 40 * 1024;

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
        let mut txc = tx.clone();
        let url = url.clone();

        spawn_local(async move {
            let fut = async move {
                let file = File::open(extract_file_url(&url)).await?;
                let meta = file.metadata().await?;

                let mut reader = BufReader::with_capacity(DEFAULT_CHUNK_SIZE, file);
                let mut buf: [u8; DEFAULT_CHUNK_SIZE] = [0; DEFAULT_CHUNK_SIZE];
                let mut remaining = meta.len();

                loop {
                    // read_exact returns EOF if there are less than DEFAULT_CHUNK_SIZE bytes to read
                    let vec = if remaining >= DEFAULT_CHUNK_SIZE as u64 {
                        let count = reader.read_exact(&mut buf).await?;
                        buf[..count].to_vec()
                    } else {
                        let mut vec = Vec::with_capacity(remaining as usize);
                        reader.read_to_end(&mut vec).await?;
                        vec
                    };
                    if vec.len() == 0 {
                        break;
                    }

                    remaining -= vec.len() as u64;
                    txc.send(Ok(TransferData::from(vec))).await?;
                }

                Ok(())
            };

            abortable_stream(fut, abort_reg, tx).await
        });

        stream
    }

    fn destination(&self, url: &Url, _: &TransferArgs) -> TransferSink<TransferData, Error> {
        let (sink, mut rx, res_tx) = TransferSink::<TransferData, Error>::create(1);
        let path = PathBuf::from(extract_file_url(&url));
        let path_c = path.clone();

        spawn_local(async move {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            log::debug!("Transferring to file: {}", path.display());
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
                log::error!("Error writing to file [{}]: {}", path_c.display(), error);
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
