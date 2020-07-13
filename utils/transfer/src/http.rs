use crate::error::{Error, HttpError};
use crate::{abortable_sink, abortable_stream};
use crate::{TransferData, TransferProvider, TransferSink, TransferStream};
use actix_http::http::Method;
use actix_rt::System;
use awc::SendClientRequest;
use bytes::Bytes;
use futures::future::ready;
use futures::{SinkExt, StreamExt, TryStreamExt};
use std::thread;
use url::Url;
use ya_client_model::activity::TransferArgs;

enum HttpAuth<'s> {
    None,
    Basic {
        username: &'s str,
        password: Option<&'s str>,
    },
}

impl<'s> From<&'s Url> for HttpAuth<'s> {
    fn from(url: &'s Url) -> Self {
        if url.username().is_empty() {
            HttpAuth::None
        } else {
            HttpAuth::Basic {
                username: url.username(),
                password: url.password(),
            }
        }
    }
}

fn request(method: Method, url: Url) -> awc::ClientRequest {
    let builder = awc::ClientBuilder::new();
    match HttpAuth::from(&url) {
        HttpAuth::None => builder,
        HttpAuth::Basic { username, password } => builder.basic_auth(username, password),
    }
    .finish()
    .request(method, url.to_string())
}

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

    fn source(&self, url: &Url, _: &TransferArgs) -> TransferStream<TransferData, Error> {
        let (stream, tx, abort_reg) = TransferStream::<TransferData, Error>::create(1);
        let txc = tx.clone();
        let url = url.clone();

        thread::spawn(move || {
            let fut = async move {
                request(Method::GET, url)
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

    fn destination(&self, url: &Url, _: &TransferArgs) -> TransferSink<TransferData, Error> {
        let method = self.upload_method.clone();
        let url = url.clone();

        let (sink, rx, res_tx) = TransferSink::<TransferData, Error>::create(1);

        thread::spawn(move || {
            let fut = async move {
                match request(method, url).send_stream(rx.map(|d| d.map(Bytes::from))) {
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
