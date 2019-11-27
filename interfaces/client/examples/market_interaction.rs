use actix_rt::System;
use awc::Client;
use futures::future::{lazy, Future};
use ya_client::{market, Error};

fn main() {
    System::new("market-interaction")
        .block_on(
            lazy(|| {
                let url = "http://localhost:5001/market-api/v1/offers";
                Client::default()
                    .post(url)
                    .send_json(&market::Offer::new(
                        serde_json::json!({"zima":"już"}),
                        "()".into(),
                    ))
                    .map_err(|e| Error::SendRequestError(e, url.into()))
                    .and_then(|mut response| response.body().from_err())
            })
            .and_then(|subscription_id| {
                let provider_subscription_id = String::from_utf8_lossy(&subscription_id);
                println!("provider subscription id: {}", provider_subscription_id);

                Client::default()
                    .post("http://localhost:5001/market-api/v1/demands")
                    .send_json(&market::Demand::new(
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
            .and_then(|v: Vec<market::OfferEvent>| {
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
