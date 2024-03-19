use crate::headers;
use crate::monitor::ResponseMonitor;
use crate::response::GsbHttpCallResponseEvent;
use crate::{message::GsbHttpCallMessage, monitor::RequestsMonitor};
use async_stream::stream;
use chrono::Utc;
use futures_core::stream::Stream;
use std::fmt::{Display, Formatter};
use thiserror::Error;
use tokio::sync::mpsc;

#[derive(Clone, Debug)]
pub struct GsbToHttpProxy<M: RequestsMonitor + 'static> {
    pub base_url: String,
    pub requests_monitor: M,
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

impl<M: RequestsMonitor> GsbToHttpProxy<M> {
    pub fn pass(
        &mut self,
        message: GsbHttpCallMessage,
    ) -> impl Stream<Item = GsbHttpCallResponseEvent> {
        let url = format!("{}{}", self.base_url, message.path);

        let (tx, mut rx) = mpsc::channel(1);

        let mut requests_monitor = self.requests_monitor.clone();
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

            // start counting here
            log::debug!("Calling {}", &url);
            let response_monitor = requests_monitor.on_request().await;
            let response = builder.send().await;
            let response =
                response.map_err(|e| GsbToHttpProxyError::ErrorInResponse(e.to_string()))?;
            let bytes = response.bytes().await.unwrap();
            response_monitor.on_response().await;

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
}

#[cfg(test)]
mod tests {
    use crate::gsb_to_http::GsbToHttpProxy;
    use crate::message::GsbHttpCallMessage;
    use crate::monitor::{RequestsMonitor, ResponseMonitor};
    use futures::StreamExt;
    use std::collections::HashMap;
    use tokio::sync::mpsc::UnboundedSender;

    #[derive(Clone, Debug)]
    struct MockMonitor {
        on_request_tx: UnboundedSender<()>,
        on_response_tx: UnboundedSender<()>,
    }

    impl RequestsMonitor for MockMonitor {
        async fn on_request(&mut self) -> impl ResponseMonitor {
            _ = self.on_request_tx.send(());
            self.clone()
        }
    }

    impl ResponseMonitor for MockMonitor {
        async fn on_response(self) {
            let _ = self.on_response_tx.send(());
        }
    }

    #[actix_web::test]
    async fn gsb_to_http_test() {
        // Mock server
        let mut server = mockito::Server::new();
        let url = server.url();

        server
            .mock("GET", "/endpoint")
            .with_status(201)
            .with_body("response")
            .create();

        let (on_request_tx, mut on_request_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let (on_response_tx, mut on_response_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let mut gsb_call = GsbToHttpProxy {
            base_url: url,
            requests_monitor: MockMonitor {
                on_request_tx,
                on_response_tx,
            },
        };

        let message = GsbHttpCallMessage {
            method: "GET".to_string(),
            path: "/endpoint".to_string(),
            body: None,
            headers: HashMap::new(),
        };

        let mut response_stream = gsb_call.pass(message);

        let mut v = vec![];
        while let Some(event) = response_stream.next().await {
            v.push(event.msg_bytes);
        }

        assert_eq!(vec!["response".as_bytes()], v);
        assert_eq!(
            1,
            on_request_rx.recv_many(&mut Vec::new(), usize::MAX).await
        );
        assert_eq!(
            1,
            on_response_rx.recv_many(&mut Vec::new(), usize::MAX).await
        );
    }
}
