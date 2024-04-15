use crate::error::HttpProxyStatusError;
use crate::headers::Headers;
use crate::message::GsbHttpCallMessage;
use actix_http::body::MessageBody;
use actix_http::header::HeaderMap;
use futures::{Stream, StreamExt};
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

    pub fn bus_addr(&mut self, bus_addr: &str) -> &Self {
        self.bus_addr = bus_addr.to_string();
        self
    }
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
    ) -> impl Stream<Item = Result<actix_web::web::Bytes, Error>> + Unpin + Sized {
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

        let stream = stream
            .map(|item| item.unwrap_or_else(|e| Err(HttpProxyStatusError::from(e))))
            .map(move |result| {
                let msg = match result {
                    Ok(r) => actix_web::web::Bytes::from(r.msg_bytes),
                    Err(e) => actix_web::web::Bytes::from(format!("Error {}", e)),
                };
                msg.try_into_bytes().map_err(|_| {
                    Error::GsbFailure("Failed to invoke GsbHttpProxy call".to_string())
                })
            });
        Box::pin(stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::rt::pin;
    use async_stream::stream;

    #[actix_web::test]
    async fn http_to_gsb_test() {
        let gsb_call = HttpToGsbProxy {
            method: "GET".to_string(),
            path: "/endpoint".to_string(),
            body: None,
            headers: HeaderMap::new(),
        };

        let stream = stream! {
            for i in 0..3 {
                let event = GsbHttpCallResponseEvent {
                    index: i,
                    timestamp: "timestamp".to_string(),
                    msg_bytes: format!("response {}", i).into_bytes()
                };
                let result = Ok(event);
                yield Ok(result)
            }
        };
        pin!(stream);
        let mut response_stream = gsb_call.pass(|_| stream);

        let mut v = vec![];
        while let Some(event) = response_stream.next().await {
            if let Ok(event) = event {
                v.push(event);
            }
        }

        assert_eq!(vec!["response 0", "response 1", "response 2"], v);
    }
}
