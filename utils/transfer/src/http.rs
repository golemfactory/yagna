use crate::error::{Error, HttpError};
use crate::{
    abortable_sink, abortable_stream, TransferData, TransferProvider, TransferSink, TransferStream,
};
use actix_http::http::Method;
use actix_rt::System;
use awc::SendClientRequest;
use futures::future::ready;
use futures::{SinkExt, StreamExt, TryStreamExt};
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
        let txc = tx.clone();

        thread::spawn(move || {
            let fut = async move {
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

            System::new("tx-http").block_on(abortable_stream(fut, abort_reg, txc))
        });

        stream
    }

    fn destination(&self, url: &Url) -> TransferSink<TransferData, Error> {
        let method = self.upload_method.clone();
        let url = url.to_string();

        let (sink, rx, res_tx) = TransferSink::<TransferData, Error>::create(1);

        thread::spawn(move || {
            let fut = async move {
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

            System::new("rx-http").block_on(abortable_sink(fut, res_tx))
        });

        sink
    }
}
