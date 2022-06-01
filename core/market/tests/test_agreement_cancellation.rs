use chrono::{Duration, Utc};
use tokio::join;
use tokio::time::timeout;

use ya_client::model::market::agreement::State as ClientAgreementState;

use ya_client::model::market::{AgreementEventType, Reason};
use ya_market::assert_err_eq;
use ya_market::testing::{
    agreement_utils::{gen_reason, negotiate_agreement},
    proposal_util::exchange_draft_proposals,
    AgreementDaoError, AgreementError, AgreementState, ApprovalStatus, MarketsNetwork, Owner,
};

const REQ_NAME: &str = "Node-1";
const PROV_NAME: &str = "Node-2";

/// Agreement cancelled without any Provider-Requestor races.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_agreement_cancelled() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME)
        .await
        .unwrap()
        .proposal_id;

    let prov_market = network.get_market(PROV_NAME);
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);
    let prov_id = network.get_default_id(PROV_NAME);

    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::milliseconds(300),
        )
        .await
        .unwrap();

    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await
        .unwrap();

    req_engine
        .cancel_agreement(&req_id, &agreement_id, Some(gen_reason("Changed my mind")))
        .await
        .unwrap();

    let agreement = req_market
        .get_agreement(&agreement_id, &req_id)
        .await
        .unwrap();
    assert_eq!(agreement.state, ClientAgreementState::Cancelled);

    let agreement = prov_market
        .get_agreement(&agreement_id.clone().translate(Owner::Provider), &prov_id)
        .await
        .unwrap();
    assert_eq!(agreement.state, ClientAgreementState::Cancelled);
}

/// Cancelling `Approved` and `Terminated` Agreement is not allowed.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_cancel_agreement_in_wrong_state() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;

    let req_market = network.get_market(REQ_NAME);
    let req_id = network.get_default_id(REQ_NAME);

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

    let result = req_market
        .requestor_engine
        .cancel_agreement(
            &req_id,
            &negotiation.r_agreement,
            Some(gen_reason("Changed my mind")),
        )
        .await;

    assert!(result.is_err());
    assert_err_eq!(
        AgreementError::UpdateState(
            negotiation.r_agreement.clone(),
            AgreementDaoError::InvalidTransition {
                from: AgreementState::Approved,
                to: AgreementState::Cancelled
            }
        ),
        result
    );

    req_market
        .terminate_agreement(
            req_id.clone(),
            negotiation.r_agreement.clone().into_client(),
            Some(gen_reason("Failure")),
        )
        .await
        .unwrap();

    let result = req_market
        .requestor_engine
        .cancel_agreement(
            &req_id,
            &negotiation.r_agreement,
            Some(gen_reason("Changed my mind")),
        )
        .await;

    assert!(result.is_err());
    assert_err_eq!(
        AgreementError::UpdateState(
            negotiation.r_agreement.clone(),
            AgreementDaoError::InvalidTransition {
                from: AgreementState::Terminated,
                to: AgreementState::Cancelled
            }
        ),
        result
    );
}

/// `wait_for_approval` should wake up after cancelling Agreement.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_agreement_cancelled_wait_for_approval() {
    let network = MarketsNetwork::new(None)
        .await
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
    let req_id = network.get_default_id(REQ_NAME);

    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::milliseconds(1500),
        )
        .await
        .unwrap();

    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await
        .unwrap();

    let agr_id = agreement_id.clone();
    let market = req_market.clone();
    let reject_handle = tokio::task::spawn_local(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        market
            .requestor_engine
            .cancel_agreement(
                &network.get_default_id(REQ_NAME),
                &agr_id.clone(),
                Some(gen_reason("Changed my mind")),
            )
            .await
            .unwrap();
    });

    // wait_for_approval should wake up after rejection.
    let result = req_engine
        .wait_for_approval(&agreement_id, 1.4)
        .await
        .unwrap();

    assert_eq!(
        result,
        ApprovalStatus::Cancelled {
            reason: Some(Reason::new("Changed my mind"))
        }
    );

    tokio::time::timeout(Duration::milliseconds(600).to_std().unwrap(), reject_handle)
        .await
        .unwrap()
        .unwrap();
}

/// Provider sends Reject Agreement at the same time as Requestor
/// sends Cancel Agreement. Result of this operation is undefined, because
/// it depends, which call will be executed first, but both Provider and Requestor
/// should end in the same state.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_agreement_simultaneous_reject_cancel() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME)
        .await
        .unwrap()
        .proposal_id;

    let prov_market = network.get_market(PROV_NAME);
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);
    let prov_id = network.get_default_id(PROV_NAME);

    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::milliseconds(300),
        )
        .await
        .unwrap();

    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await
        .unwrap();

    let reference_timestamp = Utc::now();

    let agr_id = agreement_id.clone().translate(Owner::Provider);
    let market = prov_market.clone();
    let reject_handle = tokio::task::spawn_local(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        market
            .provider_engine
            .reject_agreement(
                &network.get_default_id(PROV_NAME),
                &agr_id.clone(),
                Some(gen_reason("Not-interested")),
            )
            .await
            .unwrap();
    });

    let agr_id = agreement_id.clone();
    let market = prov_market.clone();
    let id = req_id.clone();
    let cancel_handle = tokio::task::spawn_local(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        market
            .requestor_engine
            .cancel_agreement(&id, &agr_id, Some(gen_reason("Changed my mind")))
            .await
            .unwrap();
    });

    let _ = join!(
        timeout(std::time::Duration::from_millis(500), reject_handle),
        timeout(std::time::Duration::from_millis(500), cancel_handle)
    );

    // We expect the same Agreement state after this operation.
    let r_agreement = req_market
        .get_agreement(&agreement_id, &req_id)
        .await
        .unwrap();

    let p_agreement = prov_market
        .get_agreement(&agreement_id.clone().translate(Owner::Provider), &prov_id)
        .await
        .unwrap();
    assert_eq!(r_agreement.state, p_agreement.state);

    // We expect that they both will get the same event.
    let p_events = prov_market
        .query_agreement_events(&None, 0.1, Some(2), reference_timestamp, &prov_id)
        .await
        .unwrap();
    assert_eq!(p_events.len(), 1);

    let r_events = req_market
        .query_agreement_events(&None, 0.5, Some(2), reference_timestamp, &req_id)
        .await
        .unwrap();
    assert_eq!(r_events.len(), 1);

    assert_eq!(p_events[0].event_type, r_events[0].event_type);

    // We expect, that `wait_for_approval` will return the same value as events.
    let result = req_engine
        .wait_for_approval(&agreement_id, 0.0)
        .await
        .unwrap();

    match result {
        ApprovalStatus::Cancelled { reason } => {
            assert_eq!(
                r_events[0].event_type,
                AgreementEventType::AgreementCancelledEvent { reason }
            )
        }
        ApprovalStatus::Rejected { reason } => {
            assert_eq!(
                r_events[0].event_type,
                AgreementEventType::AgreementRejectedEvent { reason }
            )
        }
        _ => panic!("Expected ApprovalStatus::Rejected or ApprovalStatus::Cancelled"),
    }
}

/// Provider sends Approve Agreement at the same time as Requestor
/// sends Cancel Agreement. Result of this operation is undefined, because
/// it depends, which call will be executed first, but both Provider and Requestor
/// should end in the same state.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_agreement_simultaneous_approve_cancel() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME)
        .await
        .unwrap()
        .proposal_id;

    let prov_market = network.get_market(PROV_NAME);
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);
    let prov_id = network.get_default_id(PROV_NAME);

    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::milliseconds(300),
        )
        .await
        .unwrap();

    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await
        .unwrap();

    let reference_timestamp = Utc::now();

    let agr_id = agreement_id.clone().translate(Owner::Provider);
    let market = prov_market.clone();
    let reject_handle = tokio::task::spawn_local(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        market
            .provider_engine
            .approve_agreement(
                network.get_default_id(PROV_NAME),
                &agr_id.clone(),
                None,
                1.0,
            )
            .await
            .unwrap();
    });

    let agr_id = agreement_id.clone();
    let market = prov_market.clone();
    let id = req_id.clone();
    let cancel_handle = tokio::task::spawn_local(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        market
            .requestor_engine
            .cancel_agreement(&id, &agr_id, Some(gen_reason("Changed my mind")))
            .await
            .unwrap();
    });

    let _ = join!(
        timeout(std::time::Duration::from_millis(500), reject_handle),
        timeout(std::time::Duration::from_millis(500), cancel_handle)
    );

    // We expect the same Agreement state after this operation.
    let r_agreement = req_market
        .get_agreement(&agreement_id, &req_id)
        .await
        .unwrap();

    let p_agreement = prov_market
        .get_agreement(&agreement_id.clone().translate(Owner::Provider), &prov_id)
        .await
        .unwrap();
    assert_eq!(r_agreement.state, p_agreement.state);

    // We expect that they both will get the same event.
    let p_events = prov_market
        .query_agreement_events(&None, 0.1, Some(2), reference_timestamp, &prov_id)
        .await
        .unwrap();
    assert_eq!(p_events.len(), 1);

    let r_events = req_market
        .query_agreement_events(&None, 0.5, Some(2), reference_timestamp, &req_id)
        .await
        .unwrap();
    assert_eq!(r_events.len(), 1);

    assert_eq!(p_events[0].event_type, r_events[0].event_type);

    // We expect, that `wait_for_approval` will return the same value as events.
    let result = req_engine
        .wait_for_approval(&agreement_id, 0.0)
        .await
        .unwrap();

    match result {
        ApprovalStatus::Cancelled { reason } => {
            assert_eq!(
                r_events[0].event_type,
                AgreementEventType::AgreementCancelledEvent { reason }
            )
        }
        ApprovalStatus::Approved => {
            assert_eq!(
                r_events[0].event_type,
                AgreementEventType::AgreementApprovedEvent {}
            )
        }
        _ => panic!("Expected ApprovalStatus::Approved or ApprovalStatus::Cancelled"),
    }
}
