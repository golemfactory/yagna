use crate::error::HttpProxyStatusError;
use crate::headers::Headers;
use crate::message::GsbHttpCallMessage;
use crate::response::GsbHttpCallResponseEvent;
use actix_http::body::MessageBody;
use actix_http::header::HeaderMap;
use futures::{Stream, StreamExt};
use ya_service_bus::Error;

#[derive(Clone, Debug)]
pub struct HttpToGsbProxy {
    pub method: String,
    pub path: String,
    pub body: Option<Vec<u8>>,
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

        let msg = GsbHttpCallMessage {
            method: self.method.to_string(),
            path,
            body: self.body.clone(),
            headers: Headers::default().filter(&self.headers),
        };

        let stream = trigger_stream(msg);

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
