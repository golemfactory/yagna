use crate::error::{Error, HttpError};
use crate::{flatten_result, TransferData, TransferProvider, TransferSink, TransferStream};
use actix_http::http::Method;
use actix_rt::System;
use awc::SendClientRequest;
use futures::future::{ready, Abortable};
use futures::{FutureExt, SinkExt, StreamExt, TryFutureExt, TryStreamExt};
use std::thread;
use url::Url;

pub struct HttpTransferProvider {
    upload_method: Method,
}

impl Default for HttpTransferProvider {
    fn default() -> Self {
        HttpTransferProvider {
            upload_method: Method::PUT,
        }
    }
}

impl TransferProvider<TransferData, Error> for HttpTransferProvider {
    fn schemes(&self) -> Vec<&'static str> {
        vec!["http", "https"]
    }

    fn source(&self, url: &Url) -> TransferStream<TransferData, Error> {
        let url = url.to_string();

        let (stream, tx, abort_reg) = TransferStream::<TransferData, Error>::create(1);
        let mut txc = tx.clone();

        thread::spawn(move || {
            let fut_inner = async move {
                awc::Client::new()
                    .get(url)
                    .send()
                    .await?
                    .into_stream()
                    .map_err(Error::from)
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
                    match flatten_result(r) {
                        Err(e) => {
                            let _ = txc.send(Err(e)).await;
                        }
                        _ => (),
                    }

                    txc.close_channel();
                    Result::<(), Error>::Ok(())
                });

            System::new("tx-http").block_on(fut)
        });

        stream
    }

    fn destination(&self, url: &Url) -> TransferSink<TransferData, Error> {
        let method = self.upload_method.clone();
        let url = url.to_string();

        let (sink, rx, res_tx) = TransferSink::<TransferData, Error>::create(1);

        thread::spawn(move || {
            let fut_inner = async move {
                match awc::Client::new()
                    .request(method, url)
                    .send_stream(rx.map(|d| d.map(TransferData::into_bytes)))
                {
                    SendClientRequest::Fut(fut, _, _) => {
                        fut.await.map_err(Error::from)?;
                        Ok(())
                    }
                    SendClientRequest::Err(maybe) => Err(match maybe {
                        Some(error) => HttpError::from(error).into(),
                        None => HttpError::Unspecified.into(),
                    }),
                }
            };

            let fut = fut_inner.then(|r: Result<(), Error>| async move {
                let _ = match r {
                    Err(e) => res_tx.send(Err(e)),
                    _ => res_tx.send(Ok(())),
                };

                Result::<(), Error>::Ok(())
            });

            System::new("rx-http").block_on(fut)
        });

        sink
    }
}
