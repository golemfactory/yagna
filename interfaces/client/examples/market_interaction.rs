use actix_rt::Runtime;
use awc::Client;
use futures::compat::Future01CompatExt;
use futures::prelude::*;
use serde_json;
use ya_model::market::{Demand, Offer, OfferEvent};

macro_rules! parse_body {
    ($r:tt) => {{
        let vec = $r
            .body()
            .compat()
            .await
            .expect("Response reading failed")
            .to_vec();

        String::from_utf8(vec).expect("String response decoding failed")
    }};
}

macro_rules! parse_json {
    ($r:tt) => {{
        $r.json()
            .compat()
            .await
            .expect("JSON response decoding failed")
    }};
}

async fn subscribe_provider() -> String {
    let mut response = Client::default()
        .post("http://localhost:5001/market-api/v1/offers")
        .send_json(&Offer::new(serde_json::json!({"zima":"już"}), "()".into()))
        .compat()
        .await
        .expect("Offers POST request failed");

    parse_body!(response)
}

async fn subscribe_requestor() -> String {
    let mut response = Client::default()
        .post("http://localhost:5001/market-api/v1/demands")
        .send_json(&Demand::new(
            serde_json::json!("{}"),
            "(&(zima=już))".into(),
        ))
        .compat()
        .await
        .expect("Demands POST request failed");

    parse_body!(response)
}

async fn query_demand_events(requestor_subscription_id: &String) -> Vec<OfferEvent> {
    let url = format!(
        "http://localhost:5001/market-api/v1/demands/{}/events?timeout=1&maxEvents=8",
        requestor_subscription_id
    );

    let mut response = Client::default()
        .get(&url)
        .send()
        .compat()
        .await
        .expect("Demand events GET request failed");

    parse_json!(response)
}

async fn query_market_stats() -> serde_json::Value {
    let mut response = Client::default()
        .get("http://localhost:5001/admin/marketStats")
        .send()
        .compat()
        .await
        .expect("Market stats GET request failed");

    parse_json!(response)
}

async fn interact() {
    let provider_subscription_id = subscribe_provider().await;
    println!("Provider subscription id: {}", provider_subscription_id);

    let requestor_subscription_id = subscribe_requestor().await;
    println!("Requestor subscription id: {}", requestor_subscription_id);

    let demand_events = query_demand_events(&requestor_subscription_id).await;
    println!("Demand events: {:?}", demand_events);

    let market_stats = query_market_stats().await;
    println!("Market stats: {:?}", market_stats);
}

fn main() {
    Runtime::new()
        .expect("Cannot create runtime")
        .block_on(interact().boxed_local().unit_error().compat())
        .expect("Runtime error");
}
