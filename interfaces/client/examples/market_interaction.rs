use awc::Client;
use futures::{compat::Future01CompatExt, FutureExt, TryFutureExt};
use serde_json;

use ya_client::{market::ApiClient, web::WebClient, Error, Result};
use ya_model::market::{Agreement, Demand, Offer, RequestorEvent};

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
        .collect(&requestor_subscription_id, Some(1), Some(2))
        .await?;

    println!("Got {} Requestor events. Yay!", requestor_events.len());
    if requestor_events.len() > 0 {
        let first_req_event: &RequestorEvent = &requestor_events[0];
        println!(
            "First come first served Requestor Event: {:#?}",
            first_req_event
        );

        let RequestorEvent::OfferEvent { offer, .. } = first_req_event;
        println!("First come first served: {:#?}", offer);
        let offer = offer.as_ref().unwrap();

        // test bed adjusted to fit the yaml
        let proposal = client
            .requestor()
            .get_proposal(&requestor_subscription_id, &offer.id)
            .await?;
        println!("First proposal: {:#?}", proposal);

        let a = Agreement::new(offer.id.clone(), "now".into());
        client.requestor().create_agreement(a).await?;
        println!(">>> agreement created with id: {}", offer.id);

        // TODO: test bed adjusted to fit yaml, BUT the call below is with invalid proposal id
        // (note the proposal id is different on requestor and provider side)
        let proposal = client
            .provider()
            .get_proposal(&provider_subscription_id, &proposal.offer.id)
            .await?;
        println!("First Demand proposal: {:#?}", proposal);
    }

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
        .map_err(|e| {
            println!("{}", e);
            e
        })
        .expect("Runtime error");
}
