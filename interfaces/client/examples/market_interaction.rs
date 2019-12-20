use awc::Client;
use futures::{compat::Future01CompatExt, FutureExt, TryFutureExt};
use serde_json;

use awc::error::SendRequestError;
use std::thread;
use std::time::Duration;
use ya_client::{
    market::{ApiClient, ProviderApi, RequestorApi},
    web::WebClient,
    Error, Result,
};
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

//////////////
// PROVIDER //
//////////////
async fn provider_interact(client: &ProviderApi) -> Result<()> {
    // provider - publish offer
    let offer = Offer::new(serde_json::json!({"zima":"już"}), "(&(lato=nie))".into());
    let provider_subscription_id = client.subscribe(&offer).await?;
    println!(
        "Provider subscription id: {} for\n\t {:?}",
        provider_subscription_id, &offer
    );

    // provider - get events
    let mut provider_events = vec![];

    while provider_events.is_empty() {
        provider_events = client
            .collect(&provider_subscription_id, Some(1), Some(2))
            .await?;
        println!("Provider - waiting for events");
        thread::sleep(Duration::from_millis(3000))
    }
    println!("Provider - Got {} events. Yay!", provider_events.len());

    // provider - support first event
    println!(
        "Provider - First come first served event: {:#?}",
        &provider_events[0]
    );

    match &provider_events[0] {
        // TODO: UNTESTED YET
        // provider - demand proposal received --> respond with an counter offer
        ProviderEvent::DemandEvent { demand, .. } => {
            println!(
                "SHOULD NOT HAPPEND! Provider - Got demand event: {:#?}.",
                demand
            );
            let proposal_id = &demand.as_ref().unwrap().id;
            // THIS CALL WAS NOT TESTED
            let agreement_proposal = client
                .get_proposal(&provider_subscription_id, &proposal_id)
                .await?;
            println!(
                "Provider - Wooha! Got Agreement Proposal: {:#?}. Approving...",
                agreement_proposal
            );

            let counter_proposal = Proposal::new(
                proposal_id.clone(),
                serde_json::json!({"wiosna":"kiedy?"}),
                "(&(jesień=stop))".into(),
            );
            let res = client
                .create_proposal(&counter_proposal, &provider_subscription_id, &proposal_id)
                .await?;
            println!("Provider - counter proposal created: {}", res)
        }
        // provider - agreement proposal received --> approve it
        ProviderEvent::NewAgreementEvent { agreement_id, .. } => {
            let agreement_id = agreement_id.as_ref().unwrap();
            println!(
                "Provider - Got new agreement proposal event {}.",
                agreement_id
            );

            let res = client.approve_agreement(agreement_id).await?;
            //let res = client.reject_agreement(agreement_id).await?;
            // TODO: this should return _before_ requestor.wait_for_approval
            println!("Provider - Agreement approved: {}", res);
        }
    }

    let market_stats = query_market_stats().await?;
    println!("Provider - Market stats: {:#?}", market_stats);

    println!("Provider - Unsubscribing...");
    let unsubscribe_result = client.unsubscribe(&provider_subscription_id).await?;
    println!("Provider - Unsubscribed: {}", unsubscribe_result);

    let market_stats = query_market_stats().await?;
    println!("Provider - Market stats: {:#?}", market_stats);

    Ok(())
}

//\\\\\\\\\\\//
// REQUESTOR //
//\\\\\\\\\\\//
async fn requestor_interact(client: &RequestorApi) -> Result<()> {
    thread::sleep(Duration::from_millis(300));
    // requestor - publish demand
    let demand = Demand::new(serde_json::json!({"lato":"nie"}), "(&(zima=już))".into());
    let requestor_subscription_id = client.subscribe(&demand).await?;
    println!(
        "Requestor subscription id: {} for\n\t {:?}",
        requestor_subscription_id, &demand
    );

    // requestor - get events
    let mut requestor_events = vec![];

    while requestor_events.is_empty() {
        requestor_events = client
            .collect(&requestor_subscription_id, Some(1), Some(2))
            .await?;
        println!("Requestor - waiting for events");
        thread::sleep(Duration::from_millis(3000))
    }
    println!("Requestor - Got {} events. Yay!", requestor_events.len());

    // requestor - support first event
    println!(
        "Requestor - First come first served event: {:#?}",
        &requestor_events[0]
    );
    let RequestorEvent::OfferEvent { offer, .. } = &requestor_events[0];
    let offer = offer.as_ref().unwrap();

    let proposal = client
        .get_proposal(&requestor_subscription_id, &offer.id)
        .await?;
    println!("Requestor - First agreement proposal: {:#?}", proposal);

    println!("Requestor - Creating agreement...");
    let agreement = Agreement::new(offer.id.clone(), "12/19/2019 17:43:57".into());
    client.create_agreement(&agreement).await?;
    println!(
        "Requestor - agreement created: {:?}. Confirming...",
        &agreement
    );
    client.confirm_agreement(&agreement.proposal_id).await?;
    println!("Requestor - agreement {} confirmed", &agreement.proposal_id);

    println!("Requestor - Waiting for Agreement approval...");
    match client.wait_for_approval(&agreement.proposal_id).await {
        Err(Error::SendRequestError {
            e: SendRequestError::Timeout,
            ..
        }) => {
            println!("Requestor - Timeout waiting for Agreement approval...");
            Ok(())
        }
        Ok(r) => {
            println!("Requestor - OK! Agreement approved by Provider!");
            Ok(r)
        }
        e => e,
    }?;

    let market_stats = query_market_stats().await?;
    println!("Requestor - Market stats: {:#?}", market_stats);

    println!("Requestor - Unsunscribing...");
    let unsubscribe_result = client.unsubscribe(&requestor_subscription_id).await?;
    println!("Requestor - Unsubscribed: {}", unsubscribe_result);

    let market_stats = query_market_stats().await?;
    println!("Requestor - Market stats: {:#?}", market_stats);

    Ok(())
}

async fn interact() -> Result<()> {
    let client = ApiClient::new(WebClient::builder())?;

    let (p, r) = futures::join!(
        provider_interact(client.provider()),
        requestor_interact(client.requestor())
    );

    p.and(r)
}

fn main() {
    actix_rt::System::new("test")
        .block_on(interact().boxed_local().compat())
        .unwrap_or_else(|e| println!("{:#?}", e));
}
