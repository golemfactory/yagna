use chrono::{Duration, Utc};
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::temp_dir;

use ya_client::model::market::agreement_event::AgreementTerminator;
use ya_client::model::market::{AgreementEventType, Reason};
use ya_framework_mocks::assert_err_eq;
use ya_market::testing::{AgreementError, ApprovalStatus, Owner};

use ya_framework_mocks::market::legacy::mock_node::MarketsNetwork;
use ya_framework_mocks::market::legacy::proposal_util::exchange_draft_proposals;
use ya_framework_mocks::net::MockNet;

const REQ_NAME: &str = "Node-1";
const PROV_NAME: &str = "Node-2";

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_get_agreement_termination_reason() -> anyhow::Result<()> {
    enable_logs(false);
    let dir = temp_dir!("test_get_agreement_termination_reason")?;
    let dir = dir.path();

    let network = MarketsNetwork::new_mocked(dir, MockNet::new())
        .await?
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME)
        .await
        .unwrap()
        .proposal_id;
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME).await;

    // Requestor creates agreement with 1h expiration
    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await
        .unwrap();

    // There should be no termination reason yet.
    assert_err_eq!(
        AgreementError::NotTerminated(agreement_id.clone()),
        req_market
            .get_terminate_reason(req_id.clone(), agreement_id.into_client())
            .await
    );

    // Confirms it immediately
    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await
        .unwrap();

    // And starts waiting for Agreement approval by Provider
    let agr_id = agreement_id.clone();
    let req_market_ = req_market.clone();
    let query_handle = tokio::spawn(async move {
        let approval_status = req_market_
            .requestor_engine
            .wait_for_approval(&agr_id, 0.1)
            .await
            .unwrap();

        assert_eq!(approval_status, ApprovalStatus::Approved);
    });

    // Provider approves the Agreement and waits for ack
    network
        .get_market(PROV_NAME)
        .provider_engine
        .approve_agreement(
            network.get_default_id(PROV_NAME).await,
            &agreement_id.clone().translate(Owner::Provider),
            None,
            0.1,
        )
        .await
        .unwrap();

    // Protect from eternal waiting.
    tokio::time::timeout(Duration::milliseconds(150).to_std().unwrap(), query_handle)
        .await
        .unwrap()
        .unwrap();

    // There should be no termination reason yet.
    assert_err_eq!(
        AgreementError::NotTerminated(agreement_id.clone()),
        req_market
            .get_terminate_reason(req_id.clone(), agreement_id.into_client())
            .await,
    );

    let reference_reason: Reason =
        serde_json::from_value(serde_json::json!({"ala":"ma kota","message": "coś"})).unwrap();
    req_market
        .terminate_agreement(
            req_id.clone(),
            agreement_id.into_client(),
            Some(reference_reason.clone()),
        )
        .await
        .ok();

    // Check Requestor side
    let termination = req_market
        .get_terminate_reason(req_id.clone(), agreement_id.into_client())
        .await
        .unwrap();

    assert_eq!(termination.agreement_id, agreement_id.into_client());
    matches!(
        termination.event_type,
        AgreementEventType::AgreementTerminatedEvent { .. }
    );

    if let AgreementEventType::AgreementTerminatedEvent {
        reason, terminator, ..
    } = termination.event_type
    {
        assert_eq!(reason, Some(reference_reason.clone()));
        assert_eq!(terminator, AgreementTerminator::Requestor);
    }

    // Check Provider side
    let termination = network
        .get_market(PROV_NAME)
        .get_terminate_reason(
            network.get_default_id(PROV_NAME).await,
            agreement_id.into_client(),
        )
        .await
        .unwrap();

    assert_eq!(termination.agreement_id, agreement_id.into_client());
    matches!(
        termination.event_type,
        AgreementEventType::AgreementTerminatedEvent { .. }
    );

    if let AgreementEventType::AgreementTerminatedEvent {
        reason, terminator, ..
    } = termination.event_type
    {
        assert_eq!(reason, Some(reference_reason));
        assert_eq!(terminator, AgreementTerminator::Requestor);
    }

    Ok(())
}
