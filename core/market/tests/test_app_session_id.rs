use anyhow::Result;
use chrono::{Duration, Utc};

use ya_market::testing::proposal_util::exchange_proposals_exclusive;
use ya_market::testing::MarketsNetwork;
use ya_market::testing::OwnerType;

//use ya_client::model::market::{AgreementOperationEvent as AgreementEvent, Proposal};

const REQ_NAME: &str = "Node-1";
const PROV_NAME: &str = "Node-2";

/// Create Agreements with different session ids. Query Agreement
/// events using different session ids on both Provider and Requestor side.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_session_events_filtering() -> Result<()> {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance(REQ_NAME)
        .await?
        .add_market_instance(PROV_NAME)
        .await?;

    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);
    let prov_id = network.get_default_id(PROV_NAME);
    let prov_market = network.get_market(PROV_NAME);

    let num = 4;

    let mut proposals = Vec::new();
    for i in 0..num {
        proposals.push(
            exchange_proposals_exclusive(&network, REQ_NAME, PROV_NAME, &format!("neg-{}", i))
                .await
                .unwrap()
                .proposal_id,
        );
    }

    let mut agreements = vec![];
    for proposal_id in proposals.iter() {
        let agreement_id = req_engine
            .create_agreement(
                req_id.clone(),
                &proposal_id,
                Utc::now() + Duration::hours(1),
            )
            .await
            .unwrap();
        agreements.push(agreement_id);
    }

    // Create two session names.
    let mut sessions = agreements[..num - 1]
        .iter()
        .map(|_| "session-1".to_string())
        .collect::<Vec<_>>();
    sessions.push("session-2".to_string());

    let confirm_timestamp = Utc::now();
    for (agreement_id, session_id) in agreements.iter().zip(sessions.iter()) {
        req_engine
            .confirm_agreement(req_id.clone(), &agreement_id, Some(session_id.clone()))
            .await
            .unwrap();
    }

    let ids = agreements.clone();
    let session_ids = sessions.clone();
    let query_handle = tokio::task::spawn_local(async move {
        for (agreement_id, session_id) in ids.iter().zip(session_ids.iter()) {
            prov_market
                .provider_engine
                .approve_agreement(
                    network.get_default_id(PROV_NAME),
                    &agreement_id.clone().translate(OwnerType::Provider),
                    Some(session_id.clone()),
                    0.1,
                )
                .await
                .unwrap();
        }

        let events_none = prov_market
            .requestor_engine
            .query_agreement_events(&None, 0.0, Some(10), confirm_timestamp, &prov_id)
            .await
            .unwrap();

        let events_ses1 = prov_market
            .requestor_engine
            .query_agreement_events(
                &Some("session-1".to_string()),
                0.0,
                Some(10),
                confirm_timestamp,
                &prov_id,
            )
            .await
            .unwrap();

        let events_ses2 = prov_market
            .requestor_engine
            .query_agreement_events(
                &Some("session-2".to_string()),
                0.0,
                Some(10),
                confirm_timestamp,
                &prov_id,
            )
            .await
            .unwrap();

        // We expect to get all events if we set None AppSessionId.
        assert_eq!(events_none.len(), num);
        assert_eq!(events_ses1.len(), num - 1);
        assert_eq!(events_ses2.len(), 1);

        Result::<(), anyhow::Error>::Ok(())
    });

    for agreement_id in agreements.iter() {
        req_engine
            .wait_for_approval(agreement_id, 0.2)
            .await
            .unwrap();
    }

    // All events should be ready.
    let events_none = req_engine
        .query_agreement_events(&None, 0.0, Some(10), confirm_timestamp, &req_id)
        .await
        .unwrap();

    let events_ses1 = req_engine
        .query_agreement_events(
            &Some("session-1".to_string()),
            0.0,
            Some(10),
            confirm_timestamp,
            &req_id,
        )
        .await
        .unwrap();

    let events_ses2 = req_engine
        .query_agreement_events(
            &Some("session-2".to_string()),
            0.0,
            Some(10),
            confirm_timestamp,
            &req_id,
        )
        .await
        .unwrap();

    // We expect to get all events if we set None AppSessionId.
    assert_eq!(events_none.len(), num);
    assert_eq!(events_ses1.len(), num - 1);
    assert_eq!(events_ses2.len(), 1);

    // Protect from eternal waiting.
    tokio::time::timeout(Duration::milliseconds(600).to_std()?, query_handle).await???;
    Ok(())
}
