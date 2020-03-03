use crate::error::Error;
use crate::{flatten_result, TransferData, TransferProvider, TransferSink, TransferStream};
use actix_rt::System;
use bytes::BytesMut;
use futures::future::{ready, Abortable};
use futures::{FutureExt, SinkExt, StreamExt, TryFutureExt, TryStreamExt};
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
            let fut_inner = async move {
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
            };

            let fut = Abortable::new(fut_inner, abort_reg)
                .map_err(Error::from)
                .then(|r: Result<Result<(), Error>, Error>| async move {
                    if let Err(e) = flatten_result(r) {
                        let _ = txc.send(Err(e)).await;
                    }
                    txc.close_channel();
                    Result::<(), Error>::Ok(())
                });

            System::new("tx-file").block_on(fut)
        });

        stream
    }

    fn destination(&self, url: &Url) -> TransferSink<TransferData, Error> {
        let url = url.path().to_owned();

        let (sink, mut rx, res_tx, abort_reg) = TransferSink::<TransferData, Error>::create(1);

        thread::spawn(move || {
            let fut_inner = async move {
                let mut file = File::create(url.clone()).await?;
                while let Some(result) = rx.next().await {
                    file.write_all(&result?.into_bytes()).await?;
                }

                Result::<(), Error>::Ok(())
            }
            .map_err(Error::from);

            let fut = Abortable::new(fut_inner, abort_reg)
                .map_err(Error::from)
                .then(|r: Result<Result<(), Error>, Error>| async move {
                    let _ = match flatten_result(r) {
                        Err(e) => res_tx.send(Err(e)),
                        _ => res_tx.send(Ok(())),
                    };

                    Result::<(), Error>::Ok(())
                });

            System::new("rx-file").block_on(fut)
        });

        sink
    }
}
