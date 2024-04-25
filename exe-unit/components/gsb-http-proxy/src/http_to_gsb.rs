use crate::error::HttpProxyStatusError;
use crate::headers::Headers;
use crate::message::GsbHttpCallMessage;
use actix_http::body::MessageBody;
use actix_http::header::HeaderMap;
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
}

pub struct HttpToGsbProxyResponse<T> {
    pub response_stream: T,
    pub status_code: u16,
    pub response_headers: HashMap<String, Vec<String>>,
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
    pub fn pass(
        &mut self,
        method: String,
        path: String,
        headers: HeaderMap,
        body: Option<Vec<u8>>,
    ) -> impl Stream<Item = HttpToGsbProxyResponse<Result<actix_web::web::Bytes, Error>>> + Unpin + Sized
//,
    //>
    {
        let path = if let Some(stripped_url) = path.strip_prefix('/') {
            stripped_url.to_string()
        } else {
            path
        };

        let msg = GsbHttpCallMessage {
            method,
            path,
            body,
            headers: Headers::default().filter(&headers),
        };

        let stream = match &self.binding {
            BindingMode::Local => bus::service(&self.bus_addr).call_streaming(msg),
            BindingMode::Net(binding) => ya_net::from(binding.from)
                .to(binding.to)
                .service(&self.bus_addr)
                .call_streaming(msg),
        };

        let response = stream
            .map(|item| item.unwrap_or_else(|e| Err(HttpProxyStatusError::from(e))))
            .map(move |result| {
                let err = |_| Error::GsbFailure("Failed to invoke GsbHttpProxy call".to_string());

                let msg = match result {
                    Ok(r) => HttpToGsbProxyResponse {
                        response_stream: actix_web::web::Bytes::from(r.msg_bytes)
                            .try_into_bytes()
                            .map_err(err),
                        status_code: r.status_code,
                        response_headers: r.response_headers,
                    },
                    Err(e) => HttpToGsbProxyResponse {
                        response_stream: actix_web::web::Bytes::from(format!("Error {}", e))
                            .try_into_bytes()
                            .map_err(err),
                        status_code: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                        response_headers: HashMap::new(),
                    },
                };
                msg
            });
        response

        // let response = stream
        //     .map(|item| item.unwrap_or_else(|e| Err(HttpProxyStatusError::from(e))))
        //     .map(move |result| {
        //         let err = |_| Error::GsbFailure("Failed to invoke GsbHttpProxy call".to_string());
        //
        //         let msg = match result {
        //             Ok(r) => actix_web::web::Bytes::from(r.msg_bytes)
        //                 .try_into_bytes()
        //                 .map_err(err),
        //             Err(e) => actix_web::web::Bytes::from(format!("Error {}", e))
        //                 .try_into_bytes()
        //                 .map_err(err),
        //         };
        //         msg
        //     });
        // response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http_to_gsb::BindingMode::Local;
    use crate::response::GsbHttpCallResponseEvent;
    use async_stream::stream;
    use std::collections::HashMap;

    #[actix_web::test]
    async fn http_to_gsb_test() {
        let mut gsb_call = HttpToGsbProxy::new(Local);

        bus::bind_stream(crate::BUS_ID, move |_msg: GsbHttpCallMessage| {
            Box::pin(stream! {
                for i in 0..3 {
                    let response = GsbHttpCallResponseEvent {
                        index: i,
                        timestamp: "timestamp".to_string(),
                        msg_bytes: format!("response {}", i).into_bytes(),
                        response_headers: HashMap::new(),
                        status_code: 200
                    };
                    yield Ok(response);
                }
            })
        });

        let mut response_stream = gsb_call.pass(
            "GET".to_string(),
            "/endpoint".to_string(),
            HeaderMap::new(),
            None,
        );

        let mut v = vec![];
        while let Some(event) = response_stream.next().await {
            if let Ok(event) = event.response_stream {
                v.push(event);
            }
        }

        assert_eq!(vec!["response 0", "response 1", "response 2"], v);
    }
}
