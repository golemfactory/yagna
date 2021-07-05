use std::str::FromStr;

use actix_http::encoding::Decoder;
use actix_http::http::{header, Method};
use actix_http::Payload;
use awc::SendClientRequest;
use bytes::Bytes;
use futures::{FutureExt, SinkExt, StreamExt, TryStreamExt};
use futures::future::{LocalBoxFuture, ready};
use tokio::task::spawn_local;
use url::Url;

use crate::{abortable_sink, abortable_stream, TransferState};
use crate::{TransferContext, TransferData, TransferProvider, TransferSink, TransferStream};
use crate::error::{Error, HttpError};

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

    fn source(&self, url: &Url, ctx: &TransferContext) -> TransferStream<TransferData, Error> {
        let (stream, mut tx, abort_reg) = TransferStream::<TransferData, Error>::create(1);
        let txc = tx.clone();

        let url = url.clone();
        let state = ctx.state.clone();

        spawn_local(async move {
            let fut = async move {
                if state.finished() {
                    log::debug!("Transfer already finished");
                    let _ = tx.send(Ok(TransferData::Bytes(Bytes::new()))).await;
                    return Ok(());
                }
                DownloadRequest::get(url, &state)
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

            abortable_stream(fut, abort_reg, txc).await
        });

        stream
    }

    fn destination(&self, url: &Url, _: &TransferContext) -> TransferSink<TransferData, Error> {
        let method = self.upload_method.clone();
        let url = url.clone();

        let (sink, rx, res_tx) = TransferSink::<TransferData, Error>::create(1);

        spawn_local(async move {
            let fut = async move {
                client_builder(&url)
                    .finish()
                    .request(method, url.to_string())
                    .send_stream(rx.map(|res| res.map(Bytes::from)))
                    .http_err()?
                    .await
                    .map(|_| ())
            };

            abortable_sink(fut, res_tx).await
        });

        sink
    }

    fn prepare_source<'a>(
        &self,
        url: &Url,
        ctx: &TransferContext,
    ) -> LocalBoxFuture<'a, Result<(), Error>> {
        if ctx.state.offset() == 0 {
            return futures::future::ok(()).boxed_local();
        }

        let url = url.clone();
        let state = ctx.state.clone();

        async move {
            let response = DownloadRequest::head(url).send().await?;
            let ranges = response
                .headers()
                .get_all(header::ACCEPT_RANGES)
                .any(|v| v.to_str().map(|s| s == "bytes").unwrap_or(false));
            let size: Option<u64> = response
                .headers()
                .get(header::CONTENT_LENGTH)
                .map(|v| v.to_str().ok().map(|s| u64::from_str(s).ok()).flatten())
                .flatten();

            state.set_size(size);
            if !ranges {
                log::warn!("Transfer resuming is not supported by the server");
                state.set_offset(0);
            }

            Ok(())
        }
        .boxed_local()
    }
}

fn client_builder(url: &Url) -> awc::ClientBuilder {
    let builder = awc::ClientBuilder::new();
    match HttpAuth::from(url) {
        HttpAuth::None => builder,
        HttpAuth::Basic { username, password } => builder.basic_auth(username, password),
    }
}

struct DownloadRequest {
    method: Method,
    url: Url,
    offset: u64,
    max_redirects: usize,
}

impl DownloadRequest {
    pub fn get(url: Url, state: &TransferState) -> Self {
        Self {
            method: Method::GET,
            url,
            offset: state.offset(),
            max_redirects: 10,
        }
    }

    pub fn head(url: Url) -> Self {
        Self {
            method: Method::HEAD,
            url,
            offset: 0,
            max_redirects: 10,
        }
    }

    pub async fn send(
        self,
    ) -> Result<awc::ClientResponse<Decoder<Payload>>, awc::error::SendRequestError> {
        let mut redirects = self.max_redirects;
        let mut url = self.url.to_string();

        let range = match self.offset {
            0 => None,
            off => Some(format!("bytes={}-", off)),
        };

        loop {
            let mut builder = client_builder(&self.url);
            if let Some(ref range) = range {
                builder = builder.header(header::RANGE, range.clone());
            }

            let resp = builder
                .finish()
                .request(self.method.clone(), url.clone())
                .send()
                .await?;

            let is_redirect = resp.status().is_redirection();
            if (!is_redirect) || (is_redirect && redirects == 0) {
                return Ok(resp);
            }

            match resp
                .headers()
                .get(header::LOCATION)
                .map(|v| v.to_str().ok())
                .flatten()
            {
                Some(location) => {
                    url = location.to_string();
                    redirects -= 1;
                    log::debug!("Following new HTTP download location: {}", url);
                }
                None => return Ok(resp),
            }
        }
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
        if status.is_informational() || status.is_success() || status.is_redirection() {
            Ok(self)
        } else {
            if status.is_client_error() {
                Err(HttpError::Client(status.to_string()).into())
            } else {
                Err(HttpError::Server(status.to_string()).into())
            }
        }
    }
}

impl<'a> HttpErr<LocalBoxFuture<'a, Result<awc::ClientResponse, Error>>> for SendClientRequest {
    fn http_err(self) -> Result<LocalBoxFuture<'a, Result<awc::ClientResponse, Error>>, Error> {
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
