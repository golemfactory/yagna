use crate::error::{Error, HttpError};
use crate::{abortable_sink, abortable_stream};
use crate::{TransferData, TransferProvider, TransferSink, TransferStream};
use actix_http::http::Method;
use awc::SendClientRequest;
use bytes::Bytes;
use futures::future::{ready, LocalBoxFuture};
use futures::{FutureExt, SinkExt, StreamExt, TryStreamExt};
use tokio::task::spawn_local;
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

        spawn_local(async move {
            let fut = async move {
                request(Method::GET, url)
                    .send()
                    .await?
                    .http_err()?
                    .into_stream()
                    .map_err(Error::from)
                    .forward(
                        tx.sink_map_err(Error::from)
                            .with(|b| ready(Ok(Ok(TransferData::from(b))))),
                    )
                    .await
                    .map_err(Error::from)
            };

            abortable_stream(fut, abort_reg, txc).await
        });

        stream
    }

    fn destination(&self, url: &Url, _: &TransferArgs) -> TransferSink<TransferData, Error> {
        let method = self.upload_method.clone();
        let url = url.clone();

        let (sink, rx, res_tx) = TransferSink::<TransferData, Error>::create(1);

        spawn_local(async move {
            let fut = async move {
                request(method, url)
                    .send_stream(rx.map(|res| res.map(Bytes::from)))
                    .http_err()?
                    .await
                    .map(|_| ())
            };

            abortable_sink(fut, res_tx).await
        });

        sink
    }
}

trait HttpErr<T>
where
    Self: Sized,
{
    fn http_err(self) -> Result<T, Error>;
}

impl<S> HttpErr<Self> for awc::ClientResponse<S> {
    fn http_err(self) -> Result<Self, Error> {
        let status = self.status();
        if status.is_success() {
            Ok(self)
        } else if status.is_client_error() {
            Err(HttpError::Client(status.to_string()).into())
        } else {
            Err(HttpError::Server(status.to_string()).into())
        }
    }
}

impl HttpErr<awc::ConnectResponse> for awc::ConnectResponse {
    fn http_err(self) -> Result<awc::ConnectResponse, Error> {
        match self {
            awc::ConnectResponse::Client(resp) => match resp.http_err() {
                Ok(resp) => Ok(awc::ConnectResponse::Client(resp)),
                Err(error) => Err(error),
            },
            awc::ConnectResponse::Tunnel(head, framed) => {
                if head.status.is_success() {
                    Ok(awc::ConnectResponse::Tunnel(head, framed))
                } else if head.status.is_client_error() {
                    Err(HttpError::Client(head.status.to_string()).into())
                } else {
                    Err(HttpError::Server(head.status.to_string()).into())
                }
            }
        }
    }
}

impl<'a> HttpErr<LocalBoxFuture<'a, Result<awc::ConnectResponse, Error>>> for SendClientRequest {
    fn http_err(self) -> Result<LocalBoxFuture<'a, Result<awc::ConnectResponse, Error>>, Error> {
        match self {
            SendClientRequest::Fut(fut, _, _) => {
                Ok(async move { Ok(fut.await?.http_err()?) }.boxed_local())
            }
            SendClientRequest::Err(err) => Err(err
                .map(|e| e.into())
                .unwrap_or_else(|| HttpError::Other("unspecified".into()).into())),
        }
    }
}
