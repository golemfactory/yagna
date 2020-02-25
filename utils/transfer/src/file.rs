use crate::error::Error;
use crate::{TransferData, TransferProvider, TransferSink, TransferStream};
use actix_rt::System;
use bytes::BytesMut;
use futures::future::ready;
use futures::{SinkExt, StreamExt, TryStreamExt};
use std::thread;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio_util::codec::{BytesCodec, FramedRead};
use url::ParseError;

pub struct FileTransferProvider;

impl Default for FileTransferProvider {
    fn default() -> Self {
        FileTransferProvider {}
    }
}

impl TransferProvider<TransferData, (), Error> for FileTransferProvider {
    fn supports(url: &String) -> bool {
        match url::Url::parse(&url) {
            Ok(url) => url.scheme() == "file",
            Err(err) => match err {
                ParseError::RelativeUrlWithoutBase => true,
                _ => false,
            },
        }
    }

    fn source(self, url: String) -> TransferStream<TransferData, Error> {
        let (stream, mut tx) = TransferStream::<TransferData, Error>::create(1);

        thread::spawn(move || {
            System::new("transfer-file").block_on(async move {
                let file = match File::open(url).await {
                    Ok(f) => f,
                    Err(e) => {
                        let _ = tx.send(Err(Error::from(e))).await;
                        return;
                    }
                };

                let txs = tx.clone();
                let result = FramedRead::new(file, BytesCodec::new())
                    .map_ok(BytesMut::freeze)
                    .map_err(Error::from)
                    .into_stream()
                    .forward(
                        txs.sink_map_err(Error::from)
                            .with(|b| ready(Ok(Ok(TransferData::from(b))))),
                    )
                    .await;

                if let Err(e) = result {
                    let _ = tx.send(Err(Error::from(e))).await;
                }
            })
        });

        stream
    }

    fn destination(self, url: String) -> TransferSink<TransferData, (), Error> {
        let (mut sink, mut rx) = TransferSink::<TransferData, (), Error>::create(1);

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
