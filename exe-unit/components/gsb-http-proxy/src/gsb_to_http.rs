use crate::counters::Counters;
use crate::headers;
use crate::message::GsbHttpCallMessage;
use crate::response::GsbHttpCallResponseEvent;

use ya_counters::Counter;

use async_stream::stream;
use chrono::Utc;
use futures::StreamExt;
use futures_core::stream::Stream;
use std::fmt::{Display, Formatter};
use thiserror::Error;
use tokio::sync::mpsc;
use ya_service_bus::{typed as bus, Handle};

#[derive(Clone, Debug)]
pub struct GsbToHttpProxy {
    base_url: String,
    counters: Counters,
}

#[derive(Error, Debug)]
enum GsbToHttpProxyError {
    InvalidMethod,
    ErrorInResponse(String),
}

impl Display for GsbToHttpProxyError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            GsbToHttpProxyError::InvalidMethod => write!(f, "Invalid Method"),
            GsbToHttpProxyError::ErrorInResponse(e) => write!(f, "Error in response {}", e),
        }
    }
}

impl GsbToHttpProxy {
    pub fn new(base_url: String) -> Self {
        GsbToHttpProxy {
            base_url,
            counters: Default::default(),
        }
    }

    pub fn bind(&mut self, gsb_path: &str) -> Handle {
        let mut this = self.clone();
        bus::bind_stream(gsb_path, move |message: GsbHttpCallMessage| {
            let stream = this.pass(message);
            Box::pin(stream.map(Ok))
        })
    }

    pub fn pass(
        &mut self,
        message: GsbHttpCallMessage,
    ) -> impl Stream<Item = GsbHttpCallResponseEvent> {
        let url = format!("{}{}", self.base_url, message.path);

        let (tx, mut rx) = mpsc::channel(1);

        let mut counters = self.counters.clone();
        tokio::task::spawn_local(async move {
            let client = reqwest::Client::new();

            let method = actix_http::Method::from_bytes(message.method.to_uppercase().as_bytes())
                .map_err(|_| GsbToHttpProxyError::InvalidMethod)?;
            let mut builder = client.request(method, &url);

            builder = match message.body {
                Some(body) => builder.body(body),
                None => builder,
            };
            builder = headers::add(builder, message.headers);

            log::debug!("Calling {}", &url);
            let response_handler = counters.on_request();
            let response = builder.send().await;
            let response =
                response.map_err(|e| GsbToHttpProxyError::ErrorInResponse(e.to_string()))?;
            let bytes = response.bytes().await.unwrap();
            response_handler.on_response();

            let response = GsbHttpCallResponseEvent {
                index: 0,
                timestamp: Utc::now().naive_local().to_string(),
                msg_bytes: bytes.to_vec(),
            };

            tx.send(response).await.unwrap();
            Ok::<(), GsbToHttpProxyError>(())
        });

        let stream = stream! {
            while let Some(event) = rx.recv().await {
                log::info!("sending GsbEvent nr {}", &event.index);
                yield event;
            }
        };

        Box::pin(stream)
    }

    pub fn requests_counter(&mut self) -> impl Counter {
        self.counters.requests_counter()
    }

    pub fn requests_duration_counter(&mut self) -> impl Counter {
        self.counters.requests_duration_counter()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::gsb_to_http::GsbToHttpProxy;
    use crate::message::GsbHttpCallMessage;
    use futures::StreamExt;
    use mockito::{Mock, ServerGuard};
    use ya_counters::Counter;

    #[actix_web::test]
    async fn gsb_to_http_test() {
        // Mock server
        #[allow(unused)]
        let (server, mock, url) = mock_server().await;

        let mut gsb_call = GsbToHttpProxy::new(url);
        let mut requests_counter = gsb_call.requests_counter();
        let mut requests_duration_counter = gsb_call.requests_duration_counter();

        let message = message();

        let mut response_stream = gsb_call.pass(message);

        let mut v = vec![];
        while let Some(event) = response_stream.next().await {
            v.push(event.msg_bytes);
        }

        assert_eq!(vec!["response".as_bytes()], v);
        assert_eq!(1.0, requests_counter.frame().unwrap());
        assert!(requests_duration_counter.frame().unwrap() > 0.0);
    }

    #[actix_web::test]
    async fn cloned_proxy_test() {
        // Mock server
        #[allow(unused)]
        let (server, mock, url) = mock_server().await;

        let mut gsb_call = GsbToHttpProxy::new(url);
        let mut requests_counter = gsb_call.requests_counter();
        let mut requests_duration_counter = gsb_call.requests_duration_counter();

        let message = message();

        // Cloned proxy should keep initialized counters
        let mut gsb_call = gsb_call.clone();

        let mut response_stream = gsb_call.pass(message);

        let mut v = vec![];
        while let Some(event) = response_stream.next().await {
            v.push(event.msg_bytes);
        }

        assert_eq!(vec!["response".as_bytes()], v);
        assert_eq!(1.0, requests_counter.frame().unwrap());
        assert!(requests_duration_counter.frame().unwrap() > 0.0);
    }

    #[actix_web::test]
    async fn multiple_concurrent_requests() {
        // Mock server
        #[allow(unused)]
        let (server, mock, url) = mock_server().await;

        let mut gsb_call = GsbToHttpProxy::new(url);
        let mut requests_counter = gsb_call.requests_counter();
        let mut requests_duration_counter = gsb_call.requests_duration_counter();

        let task_0 = run_10_requests(gsb_call.clone());
        let task_1 = run_10_requests(gsb_call.clone());

        tokio::join!(task_0, task_1);

        assert_eq!(20.0, requests_counter.frame().unwrap());
        assert!(requests_duration_counter.frame().unwrap() > 0.0);
    }

    async fn run_10_requests(mut gsb_call_proxy: GsbToHttpProxy) {
        let message = message();
        for _ in 0..10 {
            let message = message.clone();
            let mut response_stream = gsb_call_proxy.pass(message);
            let mut v = vec![];
            while let Some(event) = response_stream.next().await {
                v.push(event.msg_bytes);
            }
            assert_eq!(vec!["response".as_bytes()], v);
        }
    }

    async fn mock_server() -> (ServerGuard, Mock, String) {
        // Mock server
        let mut server = mockito::Server::new_async().await;
        let url = server.url();
        let mock = server
            .mock("GET", "/endpoint")
            .with_status(201)
            .with_body("response")
            .create();
        (server, mock, url)
    }

    fn message() -> GsbHttpCallMessage {
        GsbHttpCallMessage {
            method: "GET".to_string(),
            path: "/endpoint".to_string(),
            body: None,
            headers: HashMap::new(),
        }
    }
}
