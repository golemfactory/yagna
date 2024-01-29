use crate::error::HttpProxyStatusError;
use crate::message::GsbHttpCallMessage;
use crate::response::GsbHttpCallResponseEvent;
use actix_http::body::MessageBody;
use actix_http::header::HeaderMap;
use futures::{Stream, StreamExt};
use serde_json::{Map, Value};
use std::collections::HashMap;
use ya_service_bus::Error;

#[derive(Clone, Debug)]
pub struct HttpToGsbProxy {
    pub method: String,
    pub path: String,
    pub body: Option<Map<String, Value>>,
    pub headers: HeaderMap,
}

impl HttpToGsbProxy {
    pub fn pass<T, F>(
        &self,
        trigger_stream: F,
    ) -> impl Stream<Item = Result<actix_web::web::Bytes, Error>> + Unpin + Sized
    where
        T: Stream<Item = Result<Result<GsbHttpCallResponseEvent, HttpProxyStatusError>, Error>>
            + Unpin,
        F: FnOnce(GsbHttpCallMessage) -> T,
    {
        let path = if let Some(stripped_url) = self.path.strip_prefix('/') {
            stripped_url.to_string()
        } else {
            self.path.clone()
        };

        let mut headers = HashMap::new();

        for header in self.headers.iter() {
            headers
                .entry(header.0.to_string())
                .and_modify(|e: &mut Vec<String>| {
                    e.push(header.1.to_str().unwrap_or_default().to_string())
                })
                .or_insert_with(|| vec![header.1.to_str().unwrap_or_default().to_string()]);
        }

        let msg = GsbHttpCallMessage {
            method: self.method.to_string(),
            path,
            body: self.body.clone(),
            headers,
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
