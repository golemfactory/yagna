use actix_http::header;
use actix_http::Method;
use bytes::Bytes;
use futures::future::{ready, LocalBoxFuture};
use futures::{FutureExt, SinkExt, StreamExt, TryStreamExt};
use std::str::FromStr;
use tokio::task::spawn_local;
use url::Url;

use crate::error::Error;
use crate::sandboxed_http::{SandboxedHttpClient, SandboxedHttpResponse};
use crate::{abortable_sink, abortable_stream, TransferState};
use crate::{TransferContext, TransferData, TransferProvider, TransferSink, TransferStream};

pub struct HttpTransferProvider {
    client: SandboxedHttpClient,
    upload_method: Method,
}

impl Default for HttpTransferProvider {
    fn default() -> Self {
        Self::with_client(SandboxedHttpClient::from_env())
    }
}

impl HttpTransferProvider {
    pub fn with_client(client: SandboxedHttpClient) -> Self {
        Self {
            client,
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
        let client = self.client.clone();

        spawn_local(async move {
            let fut = async move {
                if state.finished() {
                    log::debug!("Transfer already finished");
                    let _ = tx.send(Ok(TransferData::Bytes(Bytes::new()))).await;
                    return Ok(());
                }
                DownloadRequest::get(url, &state, client)
                    .send()
                    .await?
                    .into_stream()
                    .map_err(Error::from)
                    .forward(
                        tx.sink_map_err(Error::from)
                            .with(|b| ready(Ok(Ok(TransferData::from(b))))),
                    )
                    .await
            };

            abortable_stream(fut, abort_reg, txc).await
        });

        stream
    }

    fn destination(&self, url: &Url, _: &TransferContext) -> TransferSink<TransferData, Error> {
        let method = self.upload_method.clone();
        let url = url.clone();
        let client = self.client.clone();

        let (sink, rx, res_tx) = TransferSink::<TransferData, Error>::create(1);

        spawn_local(async move {
            let fut = async move {
                client
                    .request(method, url)
                    .send_stream(rx.map(|res| res.map(Bytes::from)))
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
        let url = url.clone();
        let state = ctx.state.clone();
        let client = self.client.clone();

        async move {
            let response = DownloadRequest::head(url, client).send().await?;
            let ranges = response
                .headers()
                .get_all(header::ACCEPT_RANGES)
                .any(|v| v.to_str().map(|s| s == "bytes").unwrap_or(false));
            let size: Option<u64> = response
                .headers()
                .get(header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok().and_then(|s| u64::from_str(s).ok()));

            match &size {
                None => log::info!("File size unknown. Http source server didn't respond with CONTENT_LENGTH header."),
                Some(size) => log::info!("Http source size reported by server: {size} B"),
            };

            state.set_size(size);
            if state.offset() != 0 && !ranges {
                log::warn!("Transfer resuming is not supported by the server");
                state.set_offset(0);
            }

            Ok(())
        }
        .boxed_local()
    }
}

struct DownloadRequest {
    client: SandboxedHttpClient,
    method: Method,
    url: Url,
    offset: u64,
}

impl DownloadRequest {
    pub fn get(url: Url, state: &TransferState, client: SandboxedHttpClient) -> Self {
        Self {
            client,
            method: Method::GET,
            url,
            offset: state.offset(),
        }
    }

    pub fn head(url: Url, client: SandboxedHttpClient) -> Self {
        Self {
            client,
            method: Method::HEAD,
            url,
            offset: 0,
        }
    }

    pub async fn send(self) -> Result<SandboxedHttpResponse, Error> {
        let range = match self.offset {
            0 => None,
            off => Some(format!("bytes={}-", off)),
        };

        let mut request = self.client.request(self.method, self.url);
        if let Some(range) = range {
            request = request.insert_header((header::RANGE, range))?;
        }

        request.send().await
    }
}
