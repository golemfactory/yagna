use crate::error::HttpProxyStatusError;
use crate::headers::Headers;
use crate::message::{GsbHttpCallMessage, GsbHttpCallStreamingMessage};
use crate::response::GsbHttpCallResponseStreamChunk;
use actix_http::body::MessageBody;
use actix_http::header::HeaderMap;
use actix_web::web::Bytes;
use futures::{Stream, StreamExt};
use http::StatusCode;
use std::collections::HashMap;
use ya_client_model::NodeId;
use ya_core_model::net as ya_net;
use ya_core_model::net::RemoteEndpoint;
use ya_service_bus::{typed as bus, Error};

#[derive(Clone, Debug)]
pub struct HttpToGsbProxy {
    pub binding: BindingMode,
    pub bus_addr: String,
}

impl HttpToGsbProxy {
    pub fn new(binding: BindingMode) -> Self {
        HttpToGsbProxy {
            binding,
            bus_addr: crate::BUS_ID.to_string(),
        }
    }

    pub fn bus_addr(&mut self, bus_addr: &str) -> Self {
        HttpToGsbProxy {
            binding: self.binding.clone(),
            bus_addr: bus_addr.to_string(),
        }
    }

    pub fn endpoint(&self) -> bus::Endpoint {
        match &self.binding {
            BindingMode::Local => bus::service(&self.bus_addr),
            BindingMode::Net(binding) => ya_net::from(binding.from)
                .to(binding.to)
                .service(&self.bus_addr),
        }
    }
}

pub struct HttpToGsbProxyResponse<T> {
    pub body: T,
    pub status_code: u16,
    pub response_headers: HashMap<String, Vec<String>>,
}

pub struct HttpToGsbProxyStreamingResponse<T> {
    pub status_code: u16,
    pub response_headers: HashMap<String, Vec<String>>,
    pub body: Result<T, Error>,
}

#[derive(Clone, Debug)]
pub enum BindingMode {
    Local,
    Net(NetBindingNodes),
}

#[derive(Clone, Debug)]
pub struct NetBindingNodes {
    pub from: NodeId,
    pub to: NodeId,
}

impl HttpToGsbProxy {
    pub async fn pass(
        &mut self,
        method: String,
        path: String,
        headers: HeaderMap,
        body: Option<Vec<u8>>,
    ) -> HttpToGsbProxyResponse<Result<Bytes, Error>> {
        let path = if let Some(stripped_url) = path.strip_prefix('/') {
            stripped_url.to_string()
        } else {
            path
        };

        let msg = GsbHttpCallMessage {
            method: method.clone(),
            path: path.clone(),
            body,
            headers: Headers::default().filter(&headers),
        };

        let endpoint = self.endpoint();

        log::info!("Proxy http {msg} call to [{}]", endpoint.addr());
        let result = endpoint
            .call(msg)
            .await
            .unwrap_or_else(|e| Err(HttpProxyStatusError::from(e)));

        match result {
            Ok(r) => {
                log::info!(
                    "Http proxy: response for {method} `{path}` call to [{}]: status: {}",
                    endpoint.addr(),
                    r.header.status_code
                );
                HttpToGsbProxyResponse {
                    body: actix_web::web::Bytes::from(r.body.msg_bytes)
                        .try_into_bytes()
                        .map_err(|_| {
                            Error::GsbFailure("Failed to invoke GsbHttpProxy call".to_string())
                        }),
                    status_code: r.header.status_code,
                    response_headers: r.header.response_headers,
                }
            }
            Err(err) => {
                log::warn!(
                    "Http proxy: error calling {method} `{path}` at [{}]: error: {err}",
                    endpoint.addr()
                );
                HttpToGsbProxyResponse {
                    body: actix_web::web::Bytes::from(format!("Error: {err}"))
                        .try_into_bytes()
                        .map_err(|_| {
                            Error::GsbFailure("Failed to invoke GsbHttpProxy call".to_string())
                        }),
                    status_code: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                    response_headers: HashMap::new(),
                }
            }
        }
    }

    pub async fn pass_streaming(
        &mut self,
        method: String,
        path: String,
        headers: HeaderMap,
        body: Option<Vec<u8>>,
    ) -> HttpToGsbProxyStreamingResponse<impl Stream<Item = Result<Bytes, Error>>> {
        let path = match path.strip_prefix('/') {
            Some(stripped_url) => stripped_url.to_string(),
            None => path,
        };

        let msg = GsbHttpCallStreamingMessage {
            method,
            path,
            body,
            headers: Headers::default().filter(&headers),
        };

        let mut stream = match &self.binding {
            BindingMode::Local => bus::service(&self.bus_addr).call_streaming(msg),
            BindingMode::Net(binding) => ya_net::from(binding.from)
                .to(binding.to)
                .service(&self.bus_addr)
                .call_streaming(msg),
        };

        let stream_header = match stream.next().await {
            Some(Ok(Ok(GsbHttpCallResponseStreamChunk::Header(h)))) => h,
            _ => {
                return HttpToGsbProxyStreamingResponse {
                    status_code: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                    response_headers: Default::default(),
                    body: Err(Error::GsbFailure("Missing stream header".to_string())),
                };
            }
        };

        let body_stream = stream
            .map(|item| item.unwrap_or_else(|e| Err(HttpProxyStatusError::from(e))))
            .map(move |result| match result {
                Ok(GsbHttpCallResponseStreamChunk::Body(body)) => Ok(Bytes::from(body.msg_bytes)),
                Ok(GsbHttpCallResponseStreamChunk::Header(_)) => {
                    Err(Error::GsbFailure("Duplicate stream header".to_string()))
                }
                Err(e) => Err(Error::GsbFailure(format!("Stream error: {e}"))),
            });

        HttpToGsbProxyStreamingResponse {
            status_code: stream_header.status_code,
            response_headers: stream_header.response_headers,
            body: Ok(body_stream),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http_to_gsb::BindingMode::Local;
    use crate::response::{GsbHttpCallResponseBody, GsbHttpCallResponseHeader};
    use async_stream::stream;

    #[actix_web::test]
    async fn http_to_gsb_test() {
        let mut gsb_call = HttpToGsbProxy::new(Local);

        bus::bind_stream(crate::BUS_ID, move |_msg: GsbHttpCallStreamingMessage| {
            Box::pin(stream! {
                let header = GsbHttpCallResponseStreamChunk::Header(GsbHttpCallResponseHeader {
                    response_headers: Default::default(),
                    status_code: 200,
                });
                yield Ok(header);

                for i in 0..3 {
                    let chunk = GsbHttpCallResponseStreamChunk::Body (
                        GsbHttpCallResponseBody {
                            msg_bytes: format!("response {}", i).into_bytes(),
                        });
                    yield Ok(chunk);
                }
            })
        });

        let response = gsb_call
            .pass_streaming(
                "GET".to_string(),
                "/endpoint".to_string(),
                HeaderMap::new(),
                None,
            )
            .await;

        let mut v = vec![];
        if let Ok(mut body) = response.body {
            while let Some(Ok(event)) = body.next().await {
                v.push(event);
            }
        }

        assert_eq!(vec!["response 0", "response 1", "response 2"], v);
    }
}
