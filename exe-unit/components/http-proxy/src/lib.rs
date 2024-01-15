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

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GsbHttpCall {
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

impl RpcMessage for GsbHttpCall {
    const ID: &'static str = "GsbHttpCall";
    type Item = GsbHttpCallEvent;
    type Error = HttpProxyStatusError;
}

impl RpcStreamMessage for GsbHttpCall {
    const ID: &'static str = "GsbHttpCall";
    type Item = GsbHttpCallEvent;
    type Error = HttpProxyStatusError;
}

impl GsbHttpCall {
    pub fn execute(&mut self, url: String) -> impl Stream<Item = GsbHttpCallEvent> {
        //http://localhost:7861/
        let url = format!("{}{}", url, self.path);

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

        stream
    }

    pub fn invoke<T, F>(
        body: Option<Map<String, Value>>,
        method: Method,
        url: String,
        trigger_stream: F,
    ) -> impl Stream<Item = Result<String, Error>> + Unpin + Sized
    where
        T: Stream<
                Item = Result<
                    Result<GsbHttpCallEvent, HttpProxyStatusError>,
                    ya_service_bus::Error,
                >,
            > + Unpin,
        F: FnOnce(GsbHttpCall) -> T,
    {
        let path = if let Some(stripped_url) = url.strip_prefix('/') {
            stripped_url.to_string()
        } else {
            url
        };

        let msg = GsbHttpCall {
            method: method.to_string(),
            path,
            body,
        };

        let stream = trigger_stream(msg);

        stream
            .map(|item| item.unwrap_or_else(|e| Err(HttpProxyStatusError::from(e))))
            .map(move |result| {
                let msg = match result {
                    Ok(r) => r.msg,
                    Err(e) => format!("Error {}", e),
                };
                Ok::<String, Error>(msg)
            })
    }
}

#[cfg(test)]
mod tests {
    // use super::*;

    #[test]
    fn it_works() {}
}
