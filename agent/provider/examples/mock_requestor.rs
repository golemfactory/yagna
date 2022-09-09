
use std::{thread, time::Duration};

use ya_client::model::market::{AgreementProposal, NewDemand, RequestorEvent};
use ya_client::{market::MarketRequestorApi, web::WebClient, Error, Result};

async fn query_events(
    client: &MarketRequestorApi,
    subscription_id: &str,
) -> Result<Vec<RequestorEvent>> {
    let mut requestor_events = vec![];

    while requestor_events.is_empty() {
        requestor_events = client.collect(subscription_id, Some(1.0), Some(2)).await?;

        println!("Waiting for events");
        thread::sleep(Duration::from_millis(3000));
    }

    println!("{} events found.", requestor_events.len());
    Ok(requestor_events)
}

async fn wait_for_approval(client: &MarketRequestorApi, proposal_id: &str) {
    loop {
        println!("Waiting for Agreement approval...");

        let _ = match client.wait_for_approval(proposal_id, None).await {
            Err(Error::TimeoutError { .. }) => {
                println!("Timeout waiting for Agreement approval...");
                Ok(())
            }
            Ok(_) => {
                println!("OK! Agreement {} approved by Provider.", proposal_id);
                return;
            }
            e => e,
        };
    }
}

async fn simulate_requestor(client: MarketRequestorApi) -> Result<()> {
    let demand = NewDemand::new(serde_json::json!({}), "(&(cpu.architecture=wasm32))".into());
    let subscription_id = client.subscribe(&demand).await?;

    println!("Demand created. Subscription_id {}.", &subscription_id);

    let requestor_events = query_events(&client, &subscription_id).await?;

    match &requestor_events[0] {
        RequestorEvent::ProposalEvent {
            event_date: _,
            proposal,
        } => {
            let proposal_id = &proposal.proposal_id;

            println!("Received offer {}. Sending agreement.", &proposal_id);

            let agreement_proposal =
                AgreementProposal::new(proposal_id.clone(), chrono::Utc::now());
            let _res = client.create_agreement(&agreement_proposal).await?;

            println!("Confirm agreement {}.", &agreement_proposal.proposal_id);
            client
                .confirm_agreement(&agreement_proposal.proposal_id, None)
                .await?;

            println!(
                "Waiting for approval of agreement {}.",
                &agreement_proposal.proposal_id
            );

            wait_for_approval(&client, &agreement_proposal.proposal_id).await;
            client.unsubscribe(&subscription_id).await?;
        }
        RequestorEvent::ProposalRejectedEvent {
            proposal_id,
            reason,
            ..
        } => {
            println!(
                "Proposal rejected [{}], reason: '{:?}'",
                proposal_id, reason
            );
        }
        RequestorEvent::PropertyQueryEvent { .. } => {
            println!("Unsupported PropertyQueryEvent.");
        }
    }

    Ok(())
}

#[actix_rt::main]
async fn main() -> Result<()> {
    simulate_requestor(WebClient::builder().build().interface()?).await
}
