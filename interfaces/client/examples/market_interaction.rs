use awc::Client;
use futures::{compat::Future01CompatExt, FutureExt, TryFutureExt};
use serde_json;

use ya_client::{market::ApiClient, web::WebClient, Error, Result};
use ya_model::market::{Demand, Offer};

async fn query_market_stats() -> Result<serde_json::Value> {
    let url = "http://localhost:5001/admin/marketStats";
    Client::default()
        .get(url)
        .send()
        .compat()
        .map_err(Error::from)
        .await?
        .json()
        .compat()
        .map_err(Error::from)
        .await
}

async fn interact() -> Result<()> {
    let client = ApiClient::new(WebClient::builder())?;

    let offer = Offer::new(serde_json::json!({"zima":"już"}), "(&(lato=nie))".into());
    let provider_subscription_id = client.provider().subscribe(offer).await?;
    println!("Provider subscription id: {}", provider_subscription_id);

    let demand = Demand::new(serde_json::json!({"lato":"nie"}), "(&(zima=już))".into());
    let requestor_subscription_id = client.requestor().subscribe(demand).await?;
    println!("Requestor subscription id: {}", requestor_subscription_id);

    let requestor_events = client
        .requestor()
        .collect(&requestor_subscription_id, Some(1), Some(3))
        .await?;
    println!("Requestor events: {:#?}", requestor_events);

    let provider_events = client
        .provider()
        .collect(&provider_subscription_id, Some(1), Some(3))
        .await?;
    println!("Provider events: {:#?}", provider_events);

    let unsubscribe_result = client
        .provider()
        .unsubscribe(&provider_subscription_id)
        .await?;
    println!("unsubscribe result: {}", unsubscribe_result);

    let market_stats = query_market_stats().await?;
    println!("Market stats: {:#?}", market_stats);
    Ok(())
}

fn main() {
    actix_rt::System::new("test")
        .block_on(interact().boxed_local().compat())
        .expect("Runtime error");
}
