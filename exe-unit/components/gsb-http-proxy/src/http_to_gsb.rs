use crate::error::HttpProxyStatusError;
use crate::headers::Headers;
use crate::message::{GsbHttpCallMessage, GsbHttpCallStreamingMessage};
use crate::response::{GsbHttpCallResponseChunk, GsbHttpCallResponseHeader};
use actix_http::body::MessageBody;
use actix_http::header::HeaderMap;
use actix_web::web::Bytes;
use async_stream::stream;
use futures::{future, stream, Stream, StreamExt};
use http::StatusCode;
use std::collections::HashMap;
use std::pin::Pin;
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
    pub body: T,
    pub status_code: u16,
    pub response_headers: HashMap<String, Vec<String>>,
}

pub struct HttpToGsbProxyStreamingResponse<T> {
    pub status_code: u16,
    pub response_headers: HashMap<String, Vec<String>>,
    pub body: Pin<Box<T>>,
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
            method,
            path,
            body,
            headers: Headers::default().filter(&headers),
        };

        let response = match &self.binding {
            BindingMode::Local => bus::service(&self.bus_addr).call(msg).await,
            BindingMode::Net(binding) => {
                ya_net::from(binding.from)
                    .to(binding.to)
                    .service(&self.bus_addr)
                    .call(msg)
                    .await
            }
        };

        let result = response.unwrap_or_else(|e| Err(HttpProxyStatusError::from(e)));

        if let Ok(r) = result {
            return HttpToGsbProxyResponse {
                body: actix_web::web::Bytes::from(r.msg_bytes)
                    .try_into_bytes()
                    .map_err(|_| {
                        Error::GsbFailure("Failed to invoke GsbHttpProxy call".to_string())
                    }),
                status_code: r.status_code,
                response_headers: r.response_headers,
            };
        }

        HttpToGsbProxyResponse {
            body: actix_web::web::Bytes::from(format!("Error: {}", result.err().unwrap()))
                .try_into_bytes()
                .map_err(|_| Error::GsbFailure("Failed to invoke GsbHttpProxy call".to_string())),
            status_code: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
            response_headers: HashMap::new(),
        }
    }

    pub async fn pass_streaming(
        &mut self,
        method: String,
        path: String,
        headers: HeaderMap,
        body: Option<Vec<u8>>,
    ) -> HttpToGsbProxyStreamingResponse<impl Stream<Item = Bytes>> {
        let path = if let Some(stripped_url) = path.strip_prefix('/') {
            stripped_url.to_string()
        } else {
            path
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

        let GsbHttpCallResponseChunk::Header(header) =
            stream.next().await.unwrap().unwrap().unwrap()
        else {
            panic!("missing header")
            // let s = stream::once(future::ok(Bytes::from("Missing stream header".to_string())));
            //
            // return HttpToGsbProxyStreamingResponse {
            //     status_code: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
            //     response_headers: Default::default(),
            //      body: Box::pin(s),
            // };
        };
        // let header = match stream.next().await {
        //     Some(Ok(Ok(GsbHttpCallResponseChunk::Header(h)))) => h,
        //     _ => {
        //         return HttpToGsbProxyStreamingResponse {
        //             status_code: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
        //             response_headers: Default::default(),
        //             // body: Box::pin(stream::iter(vec![Bytes::from(
        //             //     "Missing stream header".to_string(),
        //             // )])),
        //             body: Box::pin(sss),
        //         };
        //     }
        // };

        //
        // if header.status_code > 1000 {
        //     let response = HttpToGsbProxyStreamingResponse {
        //         status_code: header.status_code,
        //         response_headers: header.response_headers,
        //         body: Box::pin(sss),
        //     };
        //     return response;
        // }

        let s = stream
            // .map(|item| item.unwrap_or_else(|e| Err(HttpProxyStatusError::from(e))))
            .map(|item| item.unwrap())
            .map(move |result| match result {
                Ok(GsbHttpCallResponseChunk::Body(body)) => Bytes::from(body.msg_bytes),
                // Ok(GsbHttpCallResponseChunk::Header(header)) => {
                //     return HttpToGsbProxyStreamingResponse {
                //         // body: Ok("duplicate header".to_string().into_bytes()),
                //         body: Box::pin(body_stream),
                //         status_code: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                //         response_headers: HashMap::new(),
                //     };
                // }
                // Err(e) => {
                //     return HttpToGsbProxyStreamingResponse {
                //         // body: Ok(Bytes::from(format!("Error {}", e))),
                //         body: Box::pin(body_stream),
                //         status_code: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                //         response_headers: HashMap::new(),
                //     };
                // }
                _ => {
                    Bytes::from("error".to_string())
                    // return HttpToGsbProxyStreamingResponse {
                    //     status_code: StatusCode::OK.as_u16(),
                    //     response_headers: Default::default(),
                    //     body: Box::pin(sss),
                    // }
                }
            });

        HttpToGsbProxyStreamingResponse {
            status_code: header.status_code,
            response_headers: header.response_headers,
            body: Box::pin(s),
        }

        // let response = stream
        //     .map(|item| item.unwrap_or_else(|e| Err(HttpProxyStatusError::from(e))))
        //     .map(move |result| {
        //         let msg = match result {
        //             Ok(GsbHttpCallResponseChunk::Header(header)) => HttpToGsbProxyResponse {
        //                 body: Ok(Bytes::new()),
        //                 status_code: header.status_code,
        //                 response_headers: header.response_headers,
        //             },
        //             Ok(GsbHttpCallResponseChunk::Body(body)) => HttpToGsbProxyResponse {
        //                 body: Ok(Bytes::from(body.msg_bytes)),
        //                 status_code: 0,
        //                 response_headers: HashMap::new(),
        //             },
        //             Err(e) => HttpToGsbProxyResponse {
        //                 body: Ok(Bytes::from(format!("Error {}", e))),
        //                 status_code: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
        //                 response_headers: HashMap::new(),
        //             },
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
