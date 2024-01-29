use actix_http::body::MessageBody;
use async_stream::stream;
use chrono::Utc;
use futures::prelude::*;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::sync::mpsc;
use ya_service_bus::RpcStreamMessage;
use ya_service_bus::{Error, RpcMessage};

pub const BUS_ID: &str = "/public/http-proxy";

#[derive(thiserror::Error, Clone, Debug, Serialize, Deserialize)]
pub enum HttpProxyStatusError {
    #[error("{0}")]
    RuntimeException(String),
}

impl From<ya_service_bus::error::Error> for HttpProxyStatusError {
    fn from(e: Error) -> Self {
        let msg = e.to_string();
        HttpProxyStatusError::RuntimeException(msg)
    }
}

#[derive(Clone, Debug)]
pub struct HttpToGsbProxy {
    pub method: String,
    pub path: String,
    pub body: Option<Map<String, Value>>,
}

#[derive(Clone, Debug)]
pub struct GsbToHttpProxy {
    pub base_url: String,
    pub method: String,
    pub path: String,
    pub body: Option<Map<String, Value>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GsbHttpCallEvent {
    pub index: usize,
    pub timestamp: String,
    pub msg: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GsbHttpCallMessage {
    pub method: String,
    pub path: String,
    pub body: Option<Map<String, Value>>,
}

impl RpcMessage for GsbHttpCallMessage {
    const ID: &'static str = "GsbHttpCallMessage";
    type Item = GsbHttpCallEvent;
    type Error = HttpProxyStatusError;
}

impl RpcStreamMessage for GsbHttpCallMessage {
    const ID: &'static str = "GsbHttpCallMessage";
    type Item = GsbHttpCallEvent;
    type Error = HttpProxyStatusError;
}

impl GsbToHttpProxy {
    pub fn execute(&mut self) -> impl Stream<Item = GsbHttpCallEvent> {
        let url = format!("{}{}", self.base_url, self.path);

        let (tx, mut rx) = mpsc::channel(24);

        let call = self.clone();

        tokio::task::spawn_local(async move {
            let client = reqwest::Client::new();

            let method = match call.method.to_uppercase().as_str() {
                "POST" => Method::POST,
                "GET" => Method::GET,
                _ => Method::GET,
            };
            let mut builder = client.request(method, &url);

            builder = match &call.body {
                Some(body) => builder.json(body),
                None => builder,
            };

            log::info!("Calling {}", &url);
            let response = builder.send().await;
            let response = match response {
                Ok(response) => response,
                Err(err) => {
                    panic!("Error {}", err);
                }
            };

            log::info!("Got response");

            let bytes = response.bytes().await.unwrap();

            let str_bytes = String::from_utf8(bytes.to_vec()).unwrap();

            let response = GsbHttpCallEvent {
                index: 0,
                timestamp: Utc::now().naive_local().to_string(),
                msg: str_bytes,
            };

            tx.send(response).await.unwrap();
        });

        let stream = stream! {
            while let Some(event) = rx.recv().await {
                log::info!("sending GsbEvent nr {}", &event.index);
                yield event;
            }
        };

        Box::pin(stream)
    }
}

impl HttpToGsbProxy {
    pub fn pass<T, F>(
        &self,
        trigger_stream: F,
    ) -> impl Stream<Item = Result<actix_web::web::Bytes, Error>> + Unpin + Sized
    where
        T: Stream<Item = Result<Result<GsbHttpCallEvent, HttpProxyStatusError>, Error>> + Unpin,
        F: FnOnce(GsbHttpCallMessage) -> T,
    {
        let path = if let Some(stripped_url) = self.path.strip_prefix('/') {
            stripped_url.to_string()
        } else {
            self.path.clone()
        };

        let msg = GsbHttpCallMessage {
            method: self.method.to_string(),
            path,
            body: self.body.clone(),
        };

        let stream = trigger_stream(msg);

        let stream = stream
            .map(|item| item.unwrap_or_else(|e| Err(HttpProxyStatusError::from(e))))
            .map(move |result| {
                let msg = match result {
                    Ok(r) => actix_web::web::Bytes::from(r.msg),
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
    use crate::HttpToGsbProxy;
    use mockito;
    use tokio::pin;

    #[actix_web::test]
    async fn gsb_proxy_execute() {
        // Mock server
        let mut server = mockito::Server::new();
        let url = server.url();

        server
            .mock("GET", "/endpoint")
            .with_status(201)
            .with_body("response")
            .create();

        let mut gsb_call = GsbToHttpProxy {
            base_url: url,
            method: "GET".to_string(),
            path: "/endpoint".to_string(),
            body: None,
        };

        let mut response_stream = gsb_call.execute();

        let mut v = vec![];
        while let Some(event) = response_stream.next().await {
            v.push(event.msg);
        }

        assert_eq!(vec!["response"], v);
    }

    #[actix_web::test]
    async fn gsb_proxy_invoke() {
        let gsb_call = HttpToGsbProxy {
            method: "GET".to_string(),
            path: "/endpoint".to_string(),
            body: None,
        };

        let stream = stream! {
            for i in 0..3 {
                let event = GsbHttpCallEvent {
                    index: i,
                    timestamp: "timestamp".to_string(),
                    msg: format!("response {}", i)
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
