use all_asserts::*;
use chrono::{Duration, Utc};

use ya_market::testing::agreement_utils::{negotiate_agreement, negotiate_agreement_with_ids};
use ya_market::testing::proposal_util::exchange_proposals_exclusive;
use ya_market::testing::MarketsNetwork;
use ya_market::testing::Owner;

use ya_client::model::market::AgreementEventType;

const REQ_NAME: &str = "Node-1";
const PROV_NAME: &str = "Node-2";

/// Create Agreements with different session ids. Query Agreement
/// events using different session ids on both Provider and Requestor side.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_session_events_filtering() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;

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
                proposal_id,
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
            .confirm_agreement(req_id.clone(), agreement_id, Some(session_id.clone()))
            .await
            .unwrap();
    }

    let ids = agreements.clone();
    let mut session_ids = sessions.clone();
    session_ids[0] = "session-2".to_string();

    let query_handle = tokio::task::spawn_local(async move {
        for (agreement_id, session_id) in ids.iter().zip(session_ids.iter()) {
            prov_market
                .provider_engine
                .approve_agreement(
                    network.get_default_id(PROV_NAME),
                    &agreement_id.clone().translate(Owner::Provider),
                    Some(session_id.clone()),
                    0.1,
                )
                .await
                .unwrap();
        }

        let events_none = prov_market
            .query_agreement_events(&None, 0.0, Some(10), confirm_timestamp, &prov_id)
            .await
            .unwrap();

        let events_ses1 = prov_market
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
            .query_agreement_events(
                &Some("session-2".to_string()),
                0.0,
                Some(10),
                confirm_timestamp,
                &prov_id,
            )
            .await
            .unwrap();

        let events_ses3 = prov_market
            .query_agreement_events(
                &Some("session-3".to_string()),
                0.0,
                Some(10),
                confirm_timestamp,
                &prov_id,
            )
            .await
            .unwrap();

        // We expect to get all events if we set None AppSessionId.
        assert_eq!(events_none.len(), num);
        assert_eq!(events_ses1.len(), num - 2);
        assert_eq!(events_ses2.len(), 2);
        assert_eq!(events_ses3.len(), 0);
    });

    for agreement_id in agreements.iter() {
        req_engine
            .wait_for_approval(agreement_id, 0.2)
            .await
            .unwrap();
    }

    // All events should be ready.
    let events_none = req_market
        .query_agreement_events(&None, 0.0, Some(10), confirm_timestamp, &req_id)
        .await
        .unwrap();

    let events_ses1 = req_market
        .query_agreement_events(
            &Some("session-1".to_string()),
            0.0,
            Some(10),
            confirm_timestamp,
            &req_id,
        )
        .await
        .unwrap();

    let events_ses2 = req_market
        .query_agreement_events(
            &Some("session-2".to_string()),
            0.0,
            Some(10),
            confirm_timestamp,
            &req_id,
        )
        .await
        .unwrap();

    let events_ses3 = req_market
        .query_agreement_events(
            &Some("session-3".to_string()),
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
    assert_eq!(events_ses3.len(), 0);

    // Protect from eternal waiting.
    tokio::time::timeout(Duration::milliseconds(600).to_std().unwrap(), query_handle)
        .await
        .unwrap()
        .unwrap();
}

/// AppSessionId isn't propagated to Provider and vice versa.
/// They are completely independent and this test checks this.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_session_should_be_independent_on_both_sides() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;

    let req_market = network.get_market(REQ_NAME);
    let req_id = network.get_default_id(REQ_NAME);
    let prov_id = network.get_default_id(PROV_NAME);
    let prov_market = network.get_market(PROV_NAME);

    let negotiation = negotiate_agreement(
        &network,
        REQ_NAME,
        PROV_NAME,
        "negotiation",
        "r-session",
        "p-session",
    )
    .await
    .unwrap();

    let confirm_timestamp = negotiation.confirm_timestamp;
    let p_events = prov_market
        .query_agreement_events(
            &Some("p-session".to_string()),
            1.0,
            Some(10),
            confirm_timestamp,
            &prov_id,
        )
        .await
        .unwrap();

    let r_events = req_market
        .query_agreement_events(
            &Some("r-session".to_string()),
            0.5,
            Some(10),
            confirm_timestamp,
            &req_id,
        )
        .await
        .unwrap();

    // Each side gets only his own event.
    assert_eq!(p_events.len(), 1);
    assert_eq!(r_events.len(), 1);
}

/// Test case, when Provider and Requestor is on the same node.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_session_negotiation_on_the_same_node() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node")
        .await;

    let req_market = network.get_market("Node");
    let req_id = network.get_default_id("Node");
    let prov_id = network.create_identity("Node", "Provider");
    let prov_market = req_market.clone();

    let negotiation = negotiate_agreement_with_ids(
        &network,
        "Node",
        "Node",
        "negotiation",
        "r-session",
        "p-session",
        &req_id,
        &prov_id,
    )
    .await
    .unwrap();

    let confirm_timestamp = negotiation.confirm_timestamp;
    let p_events = prov_market
        .query_agreement_events(
            &Some("p-session".to_string()),
            1.0,
            Some(10),
            confirm_timestamp,
            &prov_id,
        )
        .await
        .unwrap();

    let r_events = req_market
        .query_agreement_events(
            &Some("r-session".to_string()),
            0.5,
            Some(10),
            confirm_timestamp,
            &req_id,
        )
        .await
        .unwrap();

    // Each side gets only his own event.
    assert_eq!(p_events.len(), 1);
    assert_eq!(r_events.len(), 1);
}

/// Test case, when Provider and Requestor is on the same node and use the same session.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_session_negotiation_on_the_same_node_same_session() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node")
        .await;

    let req_market = network.get_market("Node");
    let req_id = network.get_default_id("Node");
    let prov_id = network.create_identity("Node", "Provider");
    let prov_market = req_market.clone();

    let negotiation = negotiate_agreement_with_ids(
        &network,
        "Node",
        "Node",
        "negotiation",
        "same-session",
        "same-session",
        &req_id,
        &prov_id,
    )
    .await
    .unwrap();

    let confirm_timestamp = negotiation.confirm_timestamp;
    let p_events = prov_market
        .query_agreement_events(
            &Some("same-session".to_string()),
            1.0,
            Some(10),
            confirm_timestamp,
            &prov_id,
        )
        .await
        .unwrap();

    let r_events = req_market
        .query_agreement_events(
            &Some("same-session".to_string()),
            0.5,
            Some(10),
            confirm_timestamp,
            &req_id,
        )
        .await
        .unwrap();

    // Because we don't distinguish between Provider and Requestor,
    // we will get events for both. Note that we use the same Identity for both.
    assert_eq!(p_events.len(), 2);
    assert_eq!(r_events.len(), 2);
}

/// We expect to get only events after specified timestamp.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_session_timestamp_filtering() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;

    let req_market = network.get_market(REQ_NAME);
    let req_id = network.get_default_id(REQ_NAME);
    let prov_id = network.get_default_id(PROV_NAME);
    let prov_market = network.get_market(PROV_NAME);

    let num_before = 4;
    let num_after = 2;
    let timestamp_before = Utc::now();

    let mut agreements = vec![];
    for i in 0..num_before {
        let negotiation = negotiate_agreement(
            &network,
            REQ_NAME,
            PROV_NAME,
            &format!("negotiation{}", i),
            "r-session",
            "p-session",
        )
        .await
        .unwrap();
        agreements.push(negotiation.r_agreement);
    }

    let timestamp_after = Utc::now();
    for i in 0..num_after {
        let negotiation = negotiate_agreement(
            &network,
            REQ_NAME,
            PROV_NAME,
            &format!("negotiation{}", i + num_before),
            "r-session",
            "p-session",
        )
        .await
        .unwrap();
        agreements.push(negotiation.r_agreement);
    }

    let timestamp_last = Utc::now();

    // Query events using oldest timestamp. We expect to get all available events.
    let p_events = prov_market
        .query_agreement_events(
            &Some("p-session".to_string()),
            0.5,
            Some(10),
            timestamp_before,
            &prov_id,
        )
        .await
        .unwrap();

    let r_events = req_market
        .query_agreement_events(
            &Some("r-session".to_string()),
            0.5,
            Some(10),
            timestamp_before,
            &req_id,
        )
        .await
        .unwrap();

    assert_eq!(p_events.len(), num_before + num_after);
    assert_eq!(r_events.len(), num_before + num_after);

    // Check if we got events for correct agreement ids.
    p_events
        .iter()
        .enumerate()
        .for_each(|(i, event)| match &event.event_type {
            AgreementEventType::AgreementApprovedEvent {} => {
                assert_eq!(event.agreement_id, agreements[i].into_client());
                assert_ge!(event.event_date, timestamp_before);
            }
            e => panic!(
                "Expected AgreementEventType::AgreementApprovedEvent, got: {:?}",
                e
            ),
        });

    r_events
        .iter()
        .enumerate()
        .for_each(|(i, event)| match &event.event_type {
            AgreementEventType::AgreementApprovedEvent {} => {
                assert_eq!(event.agreement_id, agreements[i].into_client());
                assert_ge!(event.event_date, timestamp_before);
            }
            e => panic!(
                "Expected AgreementEventType::AgreementApprovedEvent, got: {:?}",
                e
            ),
        });

    // Query events using newer timestamp. We expect to get only new events
    let p_events = prov_market
        .query_agreement_events(
            &Some("p-session".to_string()),
            0.5,
            Some(10),
            timestamp_after,
            &prov_id,
        )
        .await
        .unwrap();

    let r_events = req_market
        .query_agreement_events(
            &Some("r-session".to_string()),
            0.5,
            Some(10),
            timestamp_after,
            &req_id,
        )
        .await
        .unwrap();

    assert_eq!(p_events.len(), num_after);
    assert_eq!(r_events.len(), num_after);

    // Check if we got events for correct agreement ids.
    p_events
        .iter()
        .enumerate()
        .for_each(|(i, event)| match &event.event_type {
            AgreementEventType::AgreementApprovedEvent {} => {
                assert_eq!(event.agreement_id, agreements[num_before + i].into_client());
                assert_ge!(event.event_date, timestamp_before);
            }
            e => panic!(
                "Expected AgreementEventType::AgreementApprovedEvent, got: {:?}",
                e
            ),
        });

    r_events
        .iter()
        .enumerate()
        .for_each(|(i, event)| match &event.event_type {
            AgreementEventType::AgreementApprovedEvent {} => {
                assert_eq!(event.agreement_id, agreements[num_before + i].into_client());
                assert_ge!(event.event_date, timestamp_before);
            }
            e => panic!(
                "Expected AgreementEventType::AgreementApprovedEvent, got: {:?}",
                e
            ),
        });

    // Query events using newer last timestamp. We expect to get no events.
    let p_events = prov_market
        .query_agreement_events(
            &Some("p-session".to_string()),
            0.5,
            Some(10),
            timestamp_last,
            &prov_id,
        )
        .await
        .unwrap();

    let r_events = req_market
        .query_agreement_events(
            &Some("r-session".to_string()),
            0.5,
            Some(10),
            timestamp_last,
            &req_id,
        )
        .await
        .unwrap();

    assert_eq!(p_events.len(), 0);
    assert_eq!(r_events.len(), 0);
}

/// In the most common flow, user of the API queries events, saves timestamp
/// of the newest event and uses this timestamp in next calls.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_common_event_flow() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;

    let req_market = network.get_market(REQ_NAME);
    let req_id = network.get_default_id(REQ_NAME);

    let num: i32 = 10;
    let timestamp_before = Utc::now();

    let mut agreements = vec![];
    for i in 0..num {
        let negotiation = negotiate_agreement(
            &network,
            REQ_NAME,
            PROV_NAME,
            &format!("negotiation{}", i),
            "r-session",
            "p-session",
        )
        .await
        .unwrap();
        agreements.push(negotiation.r_agreement);
    }

    // Use max_events to query one event at the time.
    let mut current_timestamp = timestamp_before;
    for i in 0..agreements.len() {
        let events = req_market
            .query_agreement_events(
                &Some("r-session".to_string()),
                0.1,
                Some(1),
                current_timestamp,
                &req_id,
            )
            .await
            .unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].agreement_id, agreements[i].into_client());

        match &events[0].event_type {
            AgreementEventType::AgreementApprovedEvent {} => (),
            e => panic!(
                "Expected AgreementEventType::AgreementApprovedEvent, got: {:?}",
                e
            ),
        }
        current_timestamp = events[0].event_date.clone();
    }

    // We don't expect any events anymore.
    let events = req_market
        .query_agreement_events(
            &Some("r-session".to_string()),
            0.0,
            Some(1),
            current_timestamp,
            &req_id,
        )
        .await
        .unwrap();

    assert_eq!(events.len(), 0);
}
