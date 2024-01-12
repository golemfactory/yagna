use async_stream::__private::AsyncStream;
use async_stream::stream;
use chrono::Utc;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::future::Future;
use tokio::sync::mpsc;
use ya_service_bus::RpcMessage;
use ya_service_bus::RpcStreamMessage;

pub const BUS_ID: &str = "/public/http-proxy";

#[derive(thiserror::Error, Clone, Debug, Serialize, Deserialize)]
pub enum HttpProxyStatusError {
    #[error("{0}")]
    RuntimeException(String),
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
    pub fn execute(
        &mut self,
        url: String,
    ) -> AsyncStream<GsbHttpCallEvent, impl Future<Output = ()> + Sized> {
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

            loop {
                if let Some(event) =  rx.recv().await {
                    log::info!("sending GsbEvent nr {}", &event.index);
                    yield event;
                } else {
                    break;
                }
            };
        };

        stream
    }

    // pub fn invoke(
    //     stream_fun: Fn(GsbHttpCall, &str, &str),
    //     url: String,
    //     method: String,
    //     provider_id: String,
    //     activity_id: String,
    // ) {
    //     let path = if url.starts_with('/') {
    //         url[1..].to_string()
    //     } else {
    //         url
    //     };
    //
    //     let msg = GsbHttpCall { method, path, body };
    //
    //     let stream = stream_fun(msg, provider_id, activity_id);
    //
    //     let stream = stream
    //         .map(|item| match item {
    //             Ok(result) => result.map_err(|e| Error::BadRequest(e.to_string())),
    //             Err(e) => Err(Error::from(e)),
    //         })
    //         .map(move |result| {
    //             let mut bytes = BytesMut::new();
    //             let msg = match result {
    //                 Ok(r) => r.msg,
    //                 Err(e) => format!("Error {}", e),
    //             };
    //             bytes.extend_from_slice(msg.as_bytes());
    //             Ok::<Bytes, actix_web::Error>(bytes.freeze())
    //         });
    // }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {}
}
