use awc::Client;
use futures::TryFutureExt;
use serde_json;
use std::{env, thread, time::Duration};
use structopt::StructOpt;

use ya_client::{
    market::{MarketProviderApi, MarketRequestorApi},
    web::WebClient,
    Error, Result,
};
use ya_model::market::{AgreementProposal, Demand, Offer, Proposal, ProviderEvent, RequestorEvent};

#[derive(StructOpt)]
#[structopt(name = "Router", about = "Service Bus Router")]
struct Options {
    #[structopt(short = "a", long, default_value = "localhost:5001")]
    host_port: String,
    #[structopt(long, default_value = "debug")]
    log_level: String,
}

async fn query_market_stats(host_port: &String) -> Result<serde_json::Value> {
    let url = format!("http://{}/admin/marketStats", host_port);
    Client::default()
        .get(url)
        .send()
        .map_err(Error::from)
        .await?
        .json()
        .map_err(Error::from)
        .await
}

//////////////
// PROVIDER //
//////////////
async fn provider_interact(client: MarketProviderApi, host_port: &String) -> Result<()> {
    // provider - publish offer
    let offer = Offer::new(serde_json::json!({"zima":"już"}), "(&(lato=nie))".into());
    let provider_subscription_id = client.subscribe(&offer).await?;
    println!(
        "  <=PROVIDER | subscription id: {} for\n\t {:#?}",
        provider_subscription_id, &offer
    );

    let provider_subscriptions = client.get_offers().await?;

    println!(
        "PROVIDER=>  | {} active subscriptions\n\t {:#?}",
        provider_subscriptions.len(), provider_subscriptions
    );

    
    // provider - get events
    let mut provider_events = vec![];

    while provider_events.is_empty() {
        provider_events = client
            .collect(&provider_subscription_id, Some(1), Some(2))
            .await?;
        println!("  <=PROVIDER | waiting for events");
        thread::sleep(Duration::from_millis(3000))
    }
    println!(
        "  <=PROVIDER | Got {} event(s). Yay!",
        provider_events.len()
    );

    // provider - support first event
    println!(
        "  <=PROVIDER | First come first served event: {:#?}",
        &provider_events[0]
    );

    match &provider_events[0] {
        // TODO: UNTESTED YET
        // provider - demand proposal received --> respond with an counter offer
        ProviderEvent::ProposalEvent { proposal, .. } => {
            println!(
                "SHOULD NOT HAPPEND!   <=PROVIDER | Got demand event: {:#?}.",
                proposal
            );
            let proposal_id = proposal.proposal_id.as_ref().unwrap();
            // THIS CALL WAS NOT TESTED
            let agreement_proposal = client
                .get_proposal(&provider_subscription_id, &proposal_id)
                .await?;
            println!(
                "  <=PROVIDER | Wooha! Got Agreement Proposal: {:#?}. Approving...",
                agreement_proposal
            );

            let counter_proposal = Proposal::new(
                serde_json::json!({"wiosna":"kiedy?"}),
                "(&(jesień=stop))".into(),
            );
            let res = client
                .counter_proposal(&counter_proposal, &provider_subscription_id, &proposal_id)
                .await?;
            println!("  <=PROVIDER | counter proposal created: {}", res)
        }
        // provider - agreement proposal received --> approve it
        ProviderEvent::AgreementEvent { agreement, .. } => {
            let agreement_id = &agreement.agreement_id;
            println!(
                "  <=PROVIDER | Wooha! Got new Agreement event {}. Approving...",
                agreement_id
            );

            let res = client.approve_agreement(agreement_id).await?;
            //let res = client.reject_agreement(agreement_id).await?;
            // TODO: this should return _before_ requestor.wait_for_approval
            println!("  <=PROVIDER | Agreement approved: {}", res);
        }
        ProviderEvent::PropertyQueryEvent { .. } => {
            println!("Unsupported PropertyQueryEvent.");
        }
    }

    let market_stats = query_market_stats(host_port).await?;
    println!("  <=PROVIDER | Market stats: {:#?}", market_stats);

    println!("  <=PROVIDER | Unsubscribing...");
    let res = client.unsubscribe(&provider_subscription_id).await?;
    println!("  <=PROVIDER | Unsubscribed: {}", res);

    let market_stats = query_market_stats(host_port).await?;
    println!("  <=PROVIDER | Market stats: {:#?}", market_stats);

    Ok(())
}

//\\\\\\\\\\\//
// REQUESTOR //
//\\\\\\\\\\\//
async fn requestor_interact(client: MarketRequestorApi, host_port: &String) -> Result<()> {
    thread::sleep(Duration::from_millis(300));
    // requestor - publish demand
    let demand = Demand::new(serde_json::json!({"lato":"nie"}), "(&(zima=już))".into());
    let requestor_subscription_id = client.subscribe(&demand).await?;
    println!(
        "REQUESTOR=>  | subscription id: {} for\n\t {:#?}",
        requestor_subscription_id, &demand
    );

    let requestor_subscriptions = client.get_demands().await?;

    println!(
        "REQUESTOR=>  | {} active subscriptions\n\t {:#?}",
        requestor_subscriptions.len(), requestor_subscriptions
    );

    // requestor - get events
    let mut requestor_events = vec![];

    while requestor_events.is_empty() {
        requestor_events = client
            .collect(&requestor_subscription_id, Some(1), Some(2))
            .await?;
        println!("REQUESTOR=>  | waiting for events");
        thread::sleep(Duration::from_millis(3000))
    }
    println!(
        "REQUESTOR=>  | Got {} event(s). Yay!",
        requestor_events.len()
    );

    // requestor - support first event
    println!(
        "REQUESTOR=>  | First come first served event: {:#?}",
        &requestor_events[0]
    );

    match &requestor_events[0] {
        RequestorEvent::ProposalEvent { proposal, .. } => {
            let offer_id = proposal.proposal_id.as_ref().unwrap();

            // this is not needed in regular flow; just to illustrate possibility
            let proposal = client
                .get_proposal(&requestor_subscription_id, &offer_id)
                .await?;
            println!(
                "REQUESTOR=>  | Fetched first agreement proposal: {:#?}",
                proposal
            );

            println!("REQUESTOR=>  | Creating agreement...");
            let agreement = AgreementProposal::new(offer_id.clone(), chrono::Utc::now());
            let res = client.create_agreement(&agreement).await?;
            println!(
                "REQUESTOR=>  | agreement created {}: {:#?} Confirming...",
                res, &agreement
            );
            let res = client.confirm_agreement(&agreement.proposal_id).await?;
            println!(
                "REQUESTOR=>  | agreement {} confirmed: {}",
                &agreement.proposal_id, res
            );

            println!("REQUESTOR=>  | Waiting for Agreement approval...");
            match client.wait_for_approval(&agreement.proposal_id).await {
                Err(Error::TimeoutError { .. }) => {
                    println!("REQUESTOR=>  | Timeout waiting for Agreement approval...");
                    Ok("".into())
                }
                Ok(r) => {
                    println!("REQUESTOR=>  | OK! Agreement approved by Provider!: {}", r);
                    Ok(r)
                }
                e => e,
            }?;
        }
        RequestorEvent::PropertyQueryEvent { .. } => {
            println!("Unsupported PropertyQueryEvent.");
        }
    }

    let market_stats = query_market_stats(host_port).await?;
    println!("REQUESTOR=>  | Market stats: {:#?}", market_stats);

    println!("REQUESTOR=>  | Unsunscribing...");
    let res = client.unsubscribe(&requestor_subscription_id).await?;
    println!("REQUESTOR=>  | Unsubscribed: {}", res);

    let market_stats = query_market_stats(host_port).await?;
    println!("REQUESTOR=>  | Market stats: {:#?}", market_stats);

    Ok(())
}

async fn interact(host_port: &String) -> Result<()> {
    //    let client = ApiClient::new(WebClient::builder().host_port(host_port))?;
    let client = WebClient::builder().host_port(host_port).build()?;

    futures::try_join!(
        provider_interact(client.interface()?, host_port),
        requestor_interact(client.interface()?, host_port)
    )
    .map(|_| ())
}

#[actix_rt::main]
async fn main() -> Result<()> {
    let options = Options::from_args();
    println!("\nrun this example with RUST_LOG=info to see REST calls\n");
    env::set_var(
        "RUST_LOG",
        env::var("RUST_LOG").unwrap_or(options.log_level),
    );
    env_logger::init();

    interact(&options.host_port).await
}
