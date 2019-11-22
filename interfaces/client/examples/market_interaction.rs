use actix_rt::System;
use awc::Client;
use futures::future::{lazy, Future};
use ya_model::market::{Demand, Offer, OfferEvent};

fn main() {
    System::new("market-interaction")
        .block_on(
            lazy(|| {
                Client::default()
                    .post("http://localhost:5001/market-api/v1/offers")
                    .send_json(&Offer::new(serde_json::json!({"zima":"już"}), "()".into()))
                    .map_err(Error::SendRequestError)
                    .and_then(|mut response| response.body().from_err())
            })
            .and_then(|subscription_id| {
                let provider_subscription_id = String::from_utf8_lossy(&subscription_id);
                println!("provider subscription id: {}", provider_subscription_id);

                Client::default()
                    .post("http://localhost:5001/market-api/v1/demands")
                    .send_json(&Demand::new(
                        serde_json::json!("{}"),
                        "(&(zima=już))".into(),
                    ))
                    .map_err(Error::SendRequestError)
                    .and_then(|mut response| response.body().from_err())
            })
            .and_then(|subscription_id| {
                let requestor_subscription_id = String::from_utf8_lossy(&subscription_id);
                println!("requestor subscription id: {}", requestor_subscription_id);
                let url = format!(
                    "http://localhost:5001/market-api/v1/demands/{}/events?timeout=1&maxEvents=8",
                    requestor_subscription_id
                );

                Client::default()
                    .get(&url)
                    .send()
                    .map_err(Error::SendRequestError)
                    .and_then(|mut response| response.json().from_err())
            })
            .and_then(|v: Vec<OfferEvent>| {
                println!("over events ({}): {:#?}", v.len(), v);

                Client::default()
                    .get("http://localhost:5001/admin/marketStats")
                    .send()
                    .map_err(Error::SendRequestError)
                    .and_then(|mut response| response.json().from_err())
            }),
        )
        .and_then(|v: serde_json::Value| Ok(println!("market stats: {:#}", v)))
        .unwrap_or_else(|e| println!("{:#?}", e))
}

#[derive(Debug)]
pub enum Error {
    SendRequestError(awc::error::SendRequestError),
    PayloadError(awc::error::PayloadError),
    JsonPayloadError(awc::error::JsonPayloadError),
}

impl From<awc::error::SendRequestError> for Error {
    fn from(e: awc::error::SendRequestError) -> Self {
        Error::SendRequestError(e)
    }
}

impl From<awc::error::PayloadError> for Error {
    fn from(e: awc::error::PayloadError) -> Self {
        Error::PayloadError(e)
    }
}

impl From<awc::error::JsonPayloadError> for Error {
    fn from(e: awc::error::JsonPayloadError) -> Self {
        Error::JsonPayloadError(e)
    }
}
