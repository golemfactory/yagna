use crate::error::{Error, HttpError};
use crate::{TransferData, TransferProvider, TransferSink, TransferStream, TryFlatten};
use actix_http::http::Method;
use actix_rt::System;
use awc::SendClientRequest;
use futures::future::{ready, Abortable};
use futures::{SinkExt, StreamExt, TryStreamExt};
use std::thread;

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
    fn supports(scheme: &str) -> bool {
        scheme == "http" || scheme == "https"
    }

    fn source(self, url: &str) -> TransferStream<TransferData, Error> {
        let (stream, mut tx, abort_reg) = TransferStream::<TransferData, Error>::create(1);
        let url = url.to_owned();

        thread::spawn(move || {
            System::new("transfer-http").block_on(async move {
                let response = match awc::Client::new().get(url).send().await {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = tx.send(Err(Error::from(e))).await;
                        return;
                    }
                };

                let fut = response.into_stream().map_err(Error::from).forward(
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
        let (mut sink, rx) = TransferSink::<TransferData, Error>::create(1);
        let method = self.upload_method.clone();
        let url = url.to_owned();

        let fut = Box::pin(async move {
            match awc::Client::new()
                .request(method, url)
                .send_stream(rx.map(|d| d.map(TransferData::into_bytes)))
            {
                SendClientRequest::Fut(fut, _, _) => {
                    fut.await?;
                    Ok(())
                }
                SendClientRequest::Err(maybe) => match maybe {
                    Some(err) => Err(HttpError::from(err).into()),
                    None => Err(HttpError::Unspecified.into()),
                },
            }
        });

        sink.fut = Some(fut);
        sink
    }
}
