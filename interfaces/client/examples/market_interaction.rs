use awc::Client;
use futures::{compat::Future01CompatExt, FutureExt, TryFutureExt};
use serde_json;

use std::thread;
use std::time::Duration;
use ya_client::{market::ApiClient, web::WebClient, Error, Result};
use ya_model::market::{Agreement, Demand, Offer, Proposal, ProviderEvent, RequestorEvent};

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

    // provider - publish offer
    let offer = Offer::new(serde_json::json!({"zima":"już"}), "(&(lato=nie))".into());
    let provider_subscription_id = client.provider().subscribe(&offer).await?;
    println!("Provider subscription id: {} for {:?}", provider_subscription_id, &offer);

    // requestor - publish demand
    let demand = Demand::new(serde_json::json!({"lato":"nie"}), "(&(zima=już))".into());
    let requestor_subscription_id = client.requestor().subscribe(&demand).await?;
    println!("Requestor subscription id: {} for {:?}", requestor_subscription_id, &demand);

    // requestor - get events
    let mut requestor_events = vec![];

    while requestor_events.is_empty() {
        requestor_events = client
            .requestor()
            .collect(&requestor_subscription_id, Some(1), Some(2))
            .await?;
        println!("Requestor - waiting for events");
        thread::sleep(Duration::from_millis(5000))
    }
    println!("Requestor - Got {} events. Yay!", requestor_events.len());

    // requestor - support first event
    println!("Requestor - First come first served event: {:#?}", &requestor_events[0]);
    let RequestorEvent::OfferEvent { offer, .. } = &requestor_events[0];
    let offer = offer.as_ref().unwrap();

    let proposal = client
        .requestor()
        .get_proposal(&requestor_subscription_id, &offer.id)
        .await?;
    println!("Requestor - First agreement proposal: {:#?}", proposal);

    println!("Requestor - Creating agreement...");
    let agreement = Agreement::new(offer.id.clone(), "12/19/2019 17:43:57".into());
    client.requestor().create_agreement(&agreement).await?;
    println!("Requestor - agreement created: {:?}. Confirming...", &agreement);
    client.requestor().confirm_agreement(&agreement.proposal_id).await?;
    println!("Requestor - agreement {} confirmed", &agreement.proposal_id);

    // provider - get events
    let mut provider_events = vec![];

    while provider_events.is_empty() {
        provider_events = client
            .provider()
            .collect(&provider_subscription_id, Some(1), Some(2))
            .await?;
        println!("Provider - waiting for events");
        thread::sleep(Duration::from_millis(5000))
    }
    println!("Provider - Got {} events. Yay!", provider_events.len());

    // provider - support first event
    println!("Provider - First come first served event: {:#?}", &provider_events[0]);

    match &provider_events[0] {
        // provider - demand proposal received --> respond with an counter offer
        ProviderEvent::DemandEvent { demand, .. } => {
            println!("SHOULD NOT HAPPEND! Provider - Got demand event: {:#?}.", demand);
            // TODO: test bed adjusted to fit yaml, BUT the call below is with invalid proposal id
            // (note the proposal id is different on requestor and provider side)
            let propsal_id = &demand.as_ref().unwrap().id;
            let counter_proposal = Proposal::new(
                propsal_id.clone(),
                serde_json::json!({"wiosna":"kiedy?"}),
                "(&(jesień=stop))".into(),
            );
            let res = client
                .provider()
                .create_proposal(
                    &counter_proposal,
                    &provider_subscription_id,
                    &propsal_id,
                )
                .await?;
            println!("Provider - counter proposal created: {}", res)
        }
        // provider - agreement proposal received --> approve it
        ProviderEvent::NewAgreementEvent {agreement_id, ..} => {
            let agreement_id = agreement_id.as_ref().unwrap();
            println!("Provider - Got new agreement proposal event {}.", agreement_id);
//            let agreement_proposal = client
//                .provider()
//                .get_proposal(&provider_subscription_id, agreement_id)
//                .await?;
//            println!(
//                "Provider - Wooha! Got Agreement Proposal: {:#?}. Approving...",
//                agreement_proposal
//            );

            let res = client
                .provider()
                .approve_agreement(agreement_id)
                .await?;
            println!("Provider - Agreement approved {}", res);
        }
    }

    println!("Requestor - Waiting for Agreement approval...");
    client.requestor().wait_for_approval(&agreement.proposal_id).await?;
    println!("Requestor - OK! Agreement approved by Provider!");

    let market_stats = query_market_stats().await?;
    println!("Market stats: {:#?}", market_stats);

    println!("Provider - Unsubscribing...");
    let unsubscribe_result = client
        .provider()
        .unsubscribe(&provider_subscription_id)
        .await?;
    println!("Provider - Unsubscribed: {}", unsubscribe_result);

    println!("Requestor - Unsunscribing...");
    let unsubscribe_result = client
        .requestor()
        .unsubscribe(&requestor_subscription_id)
        .await?;
    println!("Requestor - Unsubscribed: {}", unsubscribe_result);

    let market_stats = query_market_stats().await?;
    println!("Market stats: {:#?}", market_stats);

    Ok(())
}

fn main() {
    actix_rt::System::new("test")
        .block_on(interact().boxed_local().compat())
        .unwrap_or_else(|e| println!("{}", e));
}
