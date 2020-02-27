use crate::error::Error;
use crate::{TransferData, TransferProvider, TransferSink, TransferStream, TryFlatten};
use actix_rt::System;
use bytes::BytesMut;
use futures::future::{ready, Abortable};
use futures::{SinkExt, StreamExt, TryStreamExt};
use std::thread;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio_util::codec::{BytesCodec, FramedRead};

pub struct FileTransferProvider;

impl Default for FileTransferProvider {
    fn default() -> Self {
        FileTransferProvider {}
    }
}

impl TransferProvider<TransferData, Error> for FileTransferProvider {
    fn supports(scheme: &str) -> bool {
        scheme == "file"
    }

    fn source(self, url: &str) -> TransferStream<TransferData, Error> {
        let (stream, mut tx, abort_reg) = TransferStream::<TransferData, Error>::create(1);
        let url = url.to_owned();

        thread::spawn(move || {
            System::new("transfer-file").block_on(async move {
                let file = match File::open(url).await {
                    Ok(f) => f,
                    Err(e) => {
                        let _ = tx.send(Err(Error::from(e))).await;
                        return;
                    }
                };

                let fut = FramedRead::new(file, BytesCodec::new())
                    .map_ok(BytesMut::freeze)
                    .map_err(Error::from)
                    .into_stream()
                    .forward(
                        tx.clone()
                            .sink_map_err(Error::from)
                            .with(|b| ready(Ok(Ok(TransferData::from(b))))),
                    );
                let result = Abortable::new(fut, abort_reg).await;

                if let Err(e) = result.try_flatten() {
                    let _ = tx.send(Err(e)).await;
                }
            })
        });

        stream
    }

    fn destination(self, url: &str) -> TransferSink<TransferData, Error> {
        let (mut sink, mut rx) = TransferSink::<TransferData, Error>::create(1);
        let url = url.to_owned();

        let fut = Box::pin(async move {
            let mut file = File::create(url).await?;
            while let Some(result) = rx.next().await {
                file.write_all(&result?.into_bytes())
                    .await
                    .map_err(Error::from)?;
            }
            Ok(())
        });

        sink.fut = Some(fut);
        sink
    }
}
