use anyhow::Result;
use chrono::{Duration, Utc};

use ya_market::testing::agreement_utils::{gen_reason, negotiate_agreement};
use ya_market::testing::events_helper::requestor::expect_approve;
use ya_market::testing::proposal_util::exchange_draft_proposals;
use ya_market::testing::MarketsNetwork;
use ya_market::testing::{ApprovalStatus, OwnerType};

use ya_client::model::market::agreement_event::AgreementTerminator;
use ya_client::model::market::AgreementOperationEvent as AgreementEvent;

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
    let prov_id = network.get_default_id(PROV_NAME);
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
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await?;

    // Provider will approve agreement after some delay.
    let agr_id = agreement_id.clone();
    let from_timestamp = confirm_timestamp.clone();
    let query_handle = tokio::task::spawn_local(async move {
        tokio::time::delay_for(std::time::Duration::from_millis(20)).await;
        prov_market
            .provider_engine
            .approve_agreement(
                network.get_default_id(PROV_NAME),
                &agr_id.clone().translate(OwnerType::Provider),
                None,
                0.1,
            )
            .await?;

        // We expect, that both Provider and Requestor will get event.
        let events = prov_market
            .query_agreement_events(&None, 0.1, Some(2), from_timestamp, &prov_id)
            .await?;

        // Expect single event
        assert_eq!(events.len(), 1);

        match &events[0] {
            AgreementEvent::AgreementApprovedEvent { agreement_id, .. } => {
                assert_eq!(agreement_id, &agr_id.into_client())
            }
            _ => panic!("Expected AgreementEvent::AgreementApprovedEvent"),
        };
        Result::<(), anyhow::Error>::Ok(())
    });

    let events = req_market
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

/// Both endpoints Agreement events and wait_for_approval should work properly.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_agreement_events_and_wait_for_approval() -> Result<()> {
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
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await?;

    let agr_id = agreement_id.clone();
    let requestor = req_market.clone();
    let wait_handle = tokio::task::spawn_local(async move {
        let status = requestor
            .requestor_engine
            .wait_for_approval(&agr_id, 60.0)
            .await
            .unwrap();
        assert_eq!(status, ApprovalStatus::Approved);
        Result::<(), anyhow::Error>::Ok(())
    });

    // Provider will approve agreement after some delay.
    let agr_id = agreement_id.clone();
    let query_handle = tokio::task::spawn_local(async move {
        tokio::time::delay_for(std::time::Duration::from_millis(20)).await;
        prov_market
            .provider_engine
            .approve_agreement(
                network.get_default_id(PROV_NAME),
                &agr_id.clone().translate(OwnerType::Provider),
                None,
                0.1,
            )
            .await?;
        Result::<(), anyhow::Error>::Ok(())
    });

    let events = req_market
        .query_agreement_events(&None, 0.5, Some(2), confirm_timestamp, &req_id)
        .await
        .unwrap();

    // Expect single event
    assert_eq!(
        expect_approve(events, 0).unwrap(),
        agreement_id.into_client()
    );

    // Protect from eternal waiting.
    tokio::time::timeout(Duration::milliseconds(600).to_std()?, query_handle).await???;
    tokio::time::timeout(Duration::milliseconds(20).to_std()?, wait_handle).await???;
    Ok(())
}

/// We expect to get AgreementTerminatedEvent on both sides Provider and Requestor
/// after terminate_agreement endpoint was called.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_agreement_terminated_event() -> Result<()> {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance(REQ_NAME)
        .await?
        .add_market_instance(PROV_NAME)
        .await?;

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

    // Take timestamp to filter AgreementApproved which should happen before.
    let reference_timestamp = Utc::now();
    prov_market
        .terminate_agreement(
            prov_id.clone(),
            negotiation.p_agreement.clone(),
            Some(gen_reason("Expired")),
        )
        .await
        .unwrap();

    // == PROVIDER
    let events = prov_market
        .query_agreement_events(&None, 0.1, Some(2), reference_timestamp, &prov_id)
        .await?;

    // Expect single event
    assert_eq!(events.len(), 1);
    match &events[0] {
        AgreementEvent::AgreementTerminatedEvent {
            agreement_id,
            terminator,
            reason,
            ..
        } => {
            assert_eq!(agreement_id, &negotiation.p_agreement.into_client());
            assert_eq!(terminator, &AgreementTerminator::Provider);
            assert_ne!(reason, &None);
            assert_eq!(reason.as_ref().unwrap().message, "Expired");
        }
        _ => panic!("Expected AgreementEvent::AgreementTerminatedEvent"),
    };

    // == REQUESTOR
    let events = req_market
        .query_agreement_events(&None, 0.1, Some(2), reference_timestamp, &req_id)
        .await?;

    // Expect single event
    assert_eq!(events.len(), 1);
    match &events[0] {
        AgreementEvent::AgreementTerminatedEvent {
            agreement_id,
            terminator,
            reason,
            ..
        } => {
            assert_eq!(agreement_id, &negotiation.r_agreement.into_client());
            assert_eq!(terminator, &AgreementTerminator::Provider);
            assert!(reason.is_some());

            assert_eq!(reason.as_ref().unwrap().message, "Expired");
        }
        _ => panic!("Expected AgreementEvent::AgreementTerminatedEvent"),
    };

    Ok(())
}
