use crate::error::{Error, HttpError};
use crate::{TransferData, TransferProvider, TransferSink, TransferStream};
use actix_http::http::Method;
use actix_rt::System;
use awc::{ClientResponse, SendClientRequest};
use futures::future::ready;
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

impl TransferProvider<TransferData, ClientResponse, Error> for HttpTransferProvider {
    fn supports(url: &String) -> bool {
        if let Ok(url) = url::Url::parse(&url) {
            let scheme = url.scheme();
            return scheme == "http" || scheme == "https";
        }
        false
    }

    fn source(self, url: String) -> TransferStream<TransferData, Error> {
        let (stream, mut tx) = TransferStream::<TransferData, Error>::create(1);

        thread::spawn(move || {
            System::new("transfer-http").block_on(async move {
                let response = match awc::Client::new().get(url).send().await {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = tx.send(Err(Error::from(e))).await;
                        return;
                    }
                };

                let txs = tx.clone();
                let result = response
                    .into_stream()
                    .map_err(Error::from)
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

    fn destination(self, url: String) -> TransferSink<TransferData, ClientResponse, Error> {
        let (mut sink, rx) = TransferSink::<TransferData, ClientResponse, Error>::create(1);
        let method = self.upload_method.clone();

        let fut = Box::pin(async move {
            match awc::Client::new()
                .request(method, url)
                .send_stream(rx.map(|t| t.map(|v| v.into_bytes())))
            {
                SendClientRequest::Fut(fut, _, _) => Ok(fut.await?),
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
