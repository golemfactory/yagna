use crate::message::GsbHttpCallMessage;
use crate::response::GsbHttpCallResponseEvent;
use async_stream::stream;
use chrono::Utc;
use futures_core::stream::Stream;
use tokio::sync::mpsc;

#[derive(Clone, Debug)]
pub struct GsbToHttpProxy {
    pub base_url: String,
}

impl GsbToHttpProxy {
    pub fn pass(
        &mut self,
        message: GsbHttpCallMessage,
    ) -> impl Stream<Item = GsbHttpCallResponseEvent> {
        let url = format!("{}{}", self.base_url, message.path);

        let (tx, mut rx) = mpsc::channel(24);

        tokio::task::spawn_local(async move {
            let client = reqwest::Client::new();

            let method = match message.method.to_uppercase().as_str() {
                "POST" => actix_http::Method::POST,
                "GET" => actix_http::Method::GET,
                _ => actix_http::Method::GET,
            };
            let mut builder = client.request(method, &url);

            builder = match &message.body {
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

            let response = GsbHttpCallResponseEvent {
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
