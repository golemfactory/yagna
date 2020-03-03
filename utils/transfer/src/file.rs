use crate::error::Error;
use crate::{TransferData, TransferProvider, TransferSink, TransferStream};
use actix_rt::System;
use bytes::BytesMut;
use futures::future::{ready, Abortable};
use futures::{FutureExt, SinkExt, StreamExt, TryStreamExt};
use std::thread;
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
        let url = url.path().to_owned();

        let (stream, tx, abort_reg) = TransferStream::<TransferData, Error>::create(1);
        let mut txc = tx.clone();

        thread::spawn(move || {
            let fut = async move {
                let file = File::open(url).await?;
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
            }
            .then(|r: Result<(), Error>| async move {
                if let Err(e) = r {
                    let _ = txc.send(Err(e)).await;
                }
                txc.close_channel();
                Result::<(), Error>::Ok(())
            });

            System::new("tx-file").block_on(Abortable::new(fut, abort_reg))
        });

        stream
    }

    fn destination(&self, url: &Url) -> TransferSink<TransferData, Error> {
        let url = url.path().to_owned();

        let (sink, mut rx, res_tx, abort_reg) = TransferSink::<TransferData, Error>::create(1);

        thread::spawn(move || {
            let fut = async move {
                let result = async move {
                    let mut file = File::create(url.clone()).await?;
                    while let Some(result) = rx.next().await {
                        file.write_all(&result?.into_bytes()).await?;
                    }
                    Ok(())
                }
                .await;

                let _ = res_tx.send(result);
            };

            System::new("rx-file").block_on(Abortable::new(fut, abort_reg))
        });

        sink
    }
}
