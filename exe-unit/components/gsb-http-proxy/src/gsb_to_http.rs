use crate::counters::Counters;
use crate::headers;
use crate::message::GsbHttpCallMessage;
use crate::response::GsbHttpCallResponseEvent;
use std::collections::HashMap;

use ya_counters::Counter;

use actix_http::Method;
use async_stream::stream;
use chrono::Utc;
use futures::StreamExt;
use futures_core::stream::Stream;
use http::StatusCode;
use reqwest::{RequestBuilder, Response};
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
pub enum GsbToHttpProxyError {
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
        bus::bind(gsb_path, move |message: GsbHttpCallMessage| {
            let mut this = this.clone();
            async move { Ok(this.pass(message).await) }
        })
    }

    pub fn bind_streaming(&mut self, gsb_path: &str) -> Handle {
        let mut this = self.clone();
        bus::bind_stream(gsb_path, move |message: GsbHttpCallMessage| {
            let stream = this.pass_streaming(message);
            Box::pin(stream.map(Ok))
        })
    }

    pub async fn pass(&mut self, message: GsbHttpCallMessage) -> GsbHttpCallResponseEvent {
        let url = format!("{}{}", self.base_url, message.path);
        log::info!("Gsb to http call - Url: {url}");

        let mut counters = self.counters.clone();

        let method = actix_http::Method::from_bytes(message.method.to_uppercase().as_bytes());
        if method.is_err() {
            return GsbHttpCallResponseEvent::with_message(
                method.err().unwrap().to_string().into_bytes(),
                StatusCode::METHOD_NOT_ALLOWED.as_u16(),
            );
        }
        let builder = Self::create_request_builder(message, &url, method.unwrap());

        log::debug!("Calling {}", &url);
        let response_handler = counters.on_request();
        let response = builder
            .send()
            .await
            .map_err(|e| GsbToHttpProxyError::ErrorInResponse(e.to_string()));

        if let Ok(response) = response {
            let response_headers = Self::collect_headers(&response);
            let status_code = response.status().as_u16();
            response_handler.on_response();
            if let Ok(bytes) = response.bytes().await {
                return GsbHttpCallResponseEvent::new(
                    0,
                    Utc::now().naive_local().to_string(),
                    bytes.to_vec(),
                    response_headers,
                    status_code,
                );
            }
        } else {
            return GsbHttpCallResponseEvent::with_message(
                response.err().unwrap().to_string().into_bytes(),
                StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
            );
        }
        return GsbHttpCallResponseEvent::with_status_code(
            StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
        );
    }

    fn create_request_builder(
        message: GsbHttpCallMessage,
        url: &String,
        method: Method,
    ) -> RequestBuilder {
        let mut builder = reqwest::Client::new().request(method, url);
        builder = match message.body {
            Some(body) => builder.body(body),
            None => builder,
        };
        builder = headers::add(builder, message.headers);
        builder
    }

    pub fn pass_streaming(
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
            let response = builder
                .send()
                .await
                .map_err(|e| GsbToHttpProxyError::ErrorInResponse(e.to_string()))?;

            let response_headers = Self::collect_headers(&response);
            let status_code = response.status();
            let mut bytes = response.bytes_stream();

            response_handler.on_response();

            while let Some(Ok(chunk)) = bytes.next().await {
                let response = GsbHttpCallResponseEvent {
                    index: 0,
                    timestamp: Utc::now().naive_local().to_string(),
                    msg_bytes: chunk.to_vec(),
                    response_headers: HashMap::new(),
                    status_code: status_code.as_u16(),
                };

                tx.send(response).await.unwrap();
            }

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

    fn collect_headers(response: &Response) -> HashMap<String, Vec<String>> {
        let mut response_headers: HashMap<String, Vec<String>> = HashMap::new();
        response
            .headers()
            .iter()
            .map(|(name, val)| {
                (
                    name.to_string(),
                    val.to_str().unwrap_or_default().to_string(),
                )
            })
            .for_each(|(h, v)| {
                response_headers
                    .entry(h.to_string())
                    .and_modify(|e: &mut Vec<String>| e.push(v.clone()))
                    .or_insert_with(|| vec![v]);
            });
        response_headers
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
        let mut headers = vec![];
        while let Some(event) = response_stream.next().await {
            v.push(event.msg_bytes);
            for (h, vals) in event.response_headers.iter() {
                vals.iter()
                    .for_each(|v| headers.push((h.to_string(), v.to_string())));
            }
        }

        assert_eq!(
            headers
                .iter()
                .any(|(h, v)| { h.eq("some-header") && v.eq("value") }),
            true
        );
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
            .with_header("Some-Header", "value")
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
