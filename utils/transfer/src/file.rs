use crate::error::Error;
use crate::{TransferData, TransferProvider, TransferSink, TransferStream, TryFlatten};
use actix_rt::System;
use bytes::BytesMut;
use futures::future::{ready, Abortable};
use futures::{FutureExt, SinkExt, StreamExt, TryStreamExt};
use std::cell::RefCell;
use std::rc::Rc;
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

        let mut stream = TransferStream::<TransferData, Error>::create(1);
        let tx = stream.tx.clone();
        let mut tx_err = tx.clone();
        let abort_reg = stream.abort_reg.take().unwrap();

        thread::spawn(move || {
            System::new("tx-file").block_on(
                async move {
                    let file = File::open(url).await?;
                    let fut = FramedRead::new(file, BytesCodec::new())
                        .map_ok(BytesMut::freeze)
                        .map_err(Error::from)
                        .into_stream()
                        .forward(
                            tx.clone()
                                .sink_map_err(Error::from)
                                .with(|b| ready(Ok(Ok(TransferData::from(b))))),
                        );

                    let result: Result<_, Error> =
                        Abortable::new(fut, abort_reg).await.try_flatten();
                    Ok(result?)
                }
                .then(|r: Result<(), Error>| async move {
                    if let Err(e) = r {
                        let _ = tx_err.send(Err(e)).await;
                    }
                    tx_err.close_channel();
                    Result::<(), Error>::Ok(())
                }),
            )
        });

        stream
    }

    fn destination(&self, url: &Url) -> TransferSink<TransferData, Error> {
        let url = url.path().to_owned();

        let mut sink = TransferSink::<TransferData, Error>::create(1);
        let rx = sink.rx.take().unwrap();
        let res_tx = sink.res_tx.take().unwrap();

        thread::spawn(move || {
            System::new("rx-file").block_on(async move {
                let rx = Rc::new(RefCell::new(rx));
                let rxc = rx.clone();

                let result = async move {
                    let mut file = match File::create(url.clone()).await {
                        Ok(file) => file,
                        Err(e) => {
                            log::error!("Unable to create a file: {:?} @ {:?}", e, url.clone());
                            return Err(Error::from(e));
                        }
                    };
                    while let Some(result) = rxc.borrow_mut().next().await {
                        file.write_all(&result?.into_bytes()).await?;
                    }

                    Ok(())
                }
                .await;

                let _ = res_tx.send(result);
                rx.borrow_mut().close();
            })
        });

        sink
    }
}
