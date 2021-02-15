use chrono::{Duration, Utc};

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
        tokio::time::delay_for(std::time::Duration::from_millis(50)).await;
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
