use crate::error::Error;
use crate::{
    abortable_sink, abortable_stream, TransferData, TransferProvider, TransferSink, TransferStream,
    UrlExt,
};
use bytes::BytesMut;
use futures::future::ready;
use futures::{SinkExt, StreamExt, TryFutureExt, TryStreamExt};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio_util::codec::{BytesCodec, FramedRead};
use url::Url;

pub struct FileTransferProvider;

impl Default for FileTransferProvider {
    fn default() -> Self {
        FileTransferProvider {}
    }
}

impl TransferProvider<TransferData, Error> for FileTransferProvider {
    fn schemes(&self) -> Vec<&'static str> {
        vec!["file"]
    }

    fn source(&self, url: &Url) -> TransferStream<TransferData, Error> {
        let (stream, tx, abort_reg) = TransferStream::<TransferData, Error>::create(1);
        let txc = tx.clone();
        let url = url.clone();

        tokio::task::spawn_local(async move {
            let fut = async move {
                let path = url.path_decoded();
                let file = File::open(&path).await?;
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

    fn destination(&self, url: &Url) -> TransferSink<TransferData, Error> {
        let (sink, mut rx, res_tx) = TransferSink::<TransferData, Error>::create(1);
        let url = url.clone();
        let url_f = url.clone();

        //TODO: Return Result from function if file creation fails.
        tokio::task::spawn_local(async move {
            let path = url_f.path_decoded();
            let fut = async move {
                let mut file = File::create(&path).await?;
                while let Some(result) = rx.next().await {
                    file.write_all(&result?.into_bytes()).await?;
                }
                file.flush().await?;

                Result::<(), Error>::Ok(())
            }
            .map_err(|error| {
                log::error!("Error opening destination file [{}]: {}", url, error);
                Error::from(error)
            });

            abortable_sink(fut, res_tx).await
        });

        sink
    }
}
