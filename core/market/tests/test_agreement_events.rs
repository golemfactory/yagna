use anyhow::Result;
use chrono::{Duration, Utc};

use ya_market::testing::proposal_util::exchange_draft_proposals;
use ya_market::testing::ya_client::model::market::event::AgreementEvent;
use ya_market::testing::MarketsNetwork;
use ya_market::testing::OwnerType;

const REQ_NAME: &str = "Node-1";
const PROV_NAME: &str = "Node-2";

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_agreement_approved_event() -> Result<()> {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance(REQ_NAME)
        .await?
        .add_market_instance(PROV_NAME)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME)
        .await?
        .proposal_id;
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);
    let prov_market = network.get_market(PROV_NAME);

    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await?;

    let confirm_timestamp = Utc::now();
    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id)
        .await?;

    // Provider will approve agreement after some delay.
    let agr_id = agreement_id.clone();
    let query_handle = tokio::task::spawn_local(async move {
        tokio::time::delay_for(std::time::Duration::from_millis(20)).await;
        prov_market
            .provider_engine
            .approve_agreement(
                network.get_default_id(PROV_NAME),
                &agr_id.clone().translate(OwnerType::Provider),
                0.1,
            )
            .await?;
        Result::<(), anyhow::Error>::Ok(())
    });

    let events = req_engine
        .query_agreement_events(&None, 0.5, Some(2), confirm_timestamp, &req_id)
        .await?;

    // Expect single event
    assert_eq!(events.len(), 1);

    let id = agreement_id.into_client();
    match &events[0] {
        AgreementEvent::AgreementApprovedEvent { agreement_id, .. } => {
            assert_eq!(agreement_id, &id)
        }
        _ => panic!("Expected AgreementEvent::AgreementApprovedEvent"),
    };

    // Protect from eternal waiting.
    tokio::time::timeout(Duration::milliseconds(600).to_std()?, query_handle).await???;
    Ok(())
}
