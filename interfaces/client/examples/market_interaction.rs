use awc::Client;
use futures::{compat::Future01CompatExt, FutureExt, TryFutureExt};
use serde_json;

use ya_client::{
    market::{ApiClient, ApiConfiguration},
    Error, Result,
};
use ya_model::market::{Demand, Offer, RequestorEvent};

macro_rules! http_client {
    ($http_method:ident, $url:expr, $payload:expr, $response_method:ident) => {{
        Client::default()
            .$http_method($url)
            .send_json($payload)
            .compat()
            .map_err(Error::from)
            .await?
            .$response_method()
            .compat()
            .map_err(Error::from)
            .await
    }};

    ($http_method:ident, $url:expr, $response_method:ident) => {{
        Client::default()
            .$http_method($url)
            .send()
            .compat()
            .map_err(Error::from)
            .await?
            .$response_method()
            .compat()
            .map_err(Error::from)
            .await
    }};
}

macro_rules! to_utf8_string {
    ($r:expr) => {{
        String::from_utf8($r.to_vec()).map_err(Error::from)
    }};
}
//
//async fn subscribe_provider() -> Result<String> {
//    let url = "http://localhost:5001/market-api/v1/offers";
//    let offer = Offer::new(serde_json::json!({"zima":"już"}), "()".into());
//    to_utf8_string!( http_client!(post, url, send_json, &offer, body )? )
//}

async fn subscribe_requestor() -> Result<String> {
    let url = "http://localhost:5001/market-api/v1/demands";
    let demand =         Demand::new(
        serde_json::json!("{}"),
        "(&(zima=już))".into(),
    );

    to_utf8_string!( http_client!(post, url, &demand, body )? )
}

async fn query_requestor_events(requestor_subscription_id: &String) -> Result<Vec<RequestorEvent>> {
    let url = format!(
        "http://localhost:5001/market-api/v1/demands/{}/events?timeout=1&maxEvents=8",
        requestor_subscription_id
    );
    http_client!(get, url, json)
}

async fn query_market_stats() -> Result<serde_json::Value> {
    let url = "http://localhost:5001/admin/marketStats";
    http_client!(get, url, json)
}

async fn interact() -> Result<()> {
    let client = ApiClient::new(ApiConfiguration::default());
    let offer = Offer::new(serde_json::json!({"zima":"już"}), "()".into());
    let provider_subscription_id = client.provider().subscribe(offer).await?;
    println!("Provider subscription id: {}", provider_subscription_id);

    let requestor_subscription_id = subscribe_requestor().await?;
    println!("Requestor subscription id: {}", requestor_subscription_id);

    let requestor_events = query_requestor_events(&requestor_subscription_id).await?;
    println!("Requestor events: {:#?}", requestor_events);

    let market_stats = query_market_stats().await?;
    println!("Market stats: {:#?}", market_stats);
    Ok(())
}

fn main() {
    actix_rt::System::new("test").block_on(interact().boxed_local().compat())
        .expect("Runtime error")
}
