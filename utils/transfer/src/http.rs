use crate::error::{Error, HttpError};
use crate::{TransferData, TransferProvider, TransferSink, TransferStream};
use actix_http::http::Method;
use actix_rt::System;
use awc::SendClientRequest;
use futures::future::{ready, Abortable};
use futures::{FutureExt, SinkExt, StreamExt, TryStreamExt};
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

        let (stream, handles) = TransferStream::<TransferData, Error>::create(1);
        let mut tx_err = handles.tx.clone();

        thread::spawn(move || {
            System::new("tx-http").block_on(
                async move {
                    let res = awc::Client::new().get(url).send().await?;
                    let fut = res.into_stream().map_err(Error::from).forward(
                        handles
                            .tx
                            .sink_map_err(Error::from)
                            .with(|b| ready(Ok(Ok(TransferData::from(b))))),
                    );

                    let result: Result<_, Error> = Abortable::new(fut, handles.abort_reg)
                        .await
                        .map_err(Error::from)?
                        .map_err(Error::from);
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
        let method = self.upload_method.clone();
        let url = url.to_string();

        let (sink, handles) = TransferSink::<TransferData, Error>::create(1);

        thread::spawn(move || {
            System::new("rx-http").block_on(async move {
                let result = match awc::Client::new()
                    .request(method, url)
                    .send_stream(handles.rx.map(|d| d.map(TransferData::into_bytes)))
                {
                    SendClientRequest::Fut(fut, _, _) => match fut.await {
                        Ok(_) => Ok(()),
                        Err(error) => Err(Error::from(error)),
                    },
                    SendClientRequest::Err(maybe) => Err(match maybe {
                        Some(error) => HttpError::from(error).into(),
                        None => HttpError::Unspecified.into(),
                    }),
                };

                let _ = handles.res_tx.send(result);
            })
        });

        sink
    }
}
