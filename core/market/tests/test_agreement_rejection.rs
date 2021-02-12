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

/// Agreement rejection happy path.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_agreement_rejected() {
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

    prov_market
        .provider_engine
        .reject_agreement(
            &prov_id,
            &agreement_id.clone().translate(Owner::Provider),
            Some(gen_reason("Not-interested")),
        )
        .await
        .unwrap();

    let agreement = req_market
        .get_agreement(&agreement_id, &req_id)
        .await
        .unwrap();
    assert_eq!(agreement.state, ClientAgreementState::Rejected);

    let agreement = prov_market
        .get_agreement(&agreement_id.clone().translate(Owner::Provider), &prov_id)
        .await
        .unwrap();
    assert_eq!(agreement.state, ClientAgreementState::Rejected);
}

/// `wait_for_approval` should wake up after rejection.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_agreement_rejected_wait_for_approval() {
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

    let agr_id = agreement_id.clone().translate(Owner::Provider);
    let reject_handle = tokio::task::spawn_local(async move {
        tokio::time::delay_for(std::time::Duration::from_millis(50)).await;
        prov_market
            .provider_engine
            .reject_agreement(
                &network.get_default_id(PROV_NAME),
                &agr_id.clone().translate(Owner::Provider),
                Some(gen_reason("Not-interested")),
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
        ApprovalStatus::Rejected {
            reason: Some(Reason::new("Not-interested"))
        }
    );

    tokio::time::timeout(Duration::milliseconds(600).to_std().unwrap(), reject_handle)
        .await
        .unwrap()
        .unwrap();
}

/// Rejecting `Approved` and `Terminated` Agreement is not allowed.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_reject_agreement_in_wrong_state() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;

    let prov_id = network.get_default_id(PROV_NAME);
    let prov_market = network.get_market(PROV_NAME);
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

    let result = prov_market
        .provider_engine
        .reject_agreement(
            &prov_id,
            &negotiation.p_agreement,
            Some(gen_reason("Not-interested")),
        )
        .await;

    assert!(result.is_err());
    assert_err_eq!(
        AgreementError::UpdateState(
            negotiation.p_agreement.clone(),
            AgreementDaoError::InvalidTransition {
                from: AgreementState::Approved,
                to: AgreementState::Rejected
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

    let result = prov_market
        .provider_engine
        .reject_agreement(
            &prov_id,
            &negotiation.p_agreement,
            Some(gen_reason("Not-interested")),
        )
        .await;

    assert!(result.is_err());
    assert_err_eq!(
        AgreementError::UpdateState(
            negotiation.p_agreement.clone(),
            AgreementDaoError::InvalidTransition {
                from: AgreementState::Terminated,
                to: AgreementState::Rejected
            }
        ),
        result
    );
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_reject_rejected_agreement() {
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

    let r_agreement = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::milliseconds(300),
        )
        .await
        .unwrap();

    req_engine
        .confirm_agreement(req_id.clone(), &r_agreement, None)
        .await
        .unwrap();

    let ref_timestamp = Utc::now();

    prov_market
        .provider_engine
        .reject_agreement(
            &prov_id,
            &r_agreement.clone().translate(Owner::Provider),
            Some(gen_reason("Not-interested")),
        )
        .await
        .unwrap();

    let p_agreement = r_agreement.clone().translate(Owner::Provider);
    let result = prov_market
        .provider_engine
        .reject_agreement(
            &prov_id,
            &p_agreement,
            Some(gen_reason("More-uninterested")),
        )
        .await;

    match result {
        Ok(_) => panic!("Reject Agreement should fail."),
        Err(AgreementError::UpdateState(
            id,
            AgreementDaoError::InvalidTransition {
                from: AgreementState::Rejected,
                to: AgreementState::Rejected,
            },
        )) => assert_eq!(id, p_agreement),
        e => panic!("Wrong error returned, got: {:?}", e),
    };

    let agreement = req_market
        .get_agreement(&r_agreement, &req_id)
        .await
        .unwrap();
    assert_eq!(agreement.state, ClientAgreementState::Rejected);

    let agreement = prov_market
        .get_agreement(&p_agreement, &prov_id)
        .await
        .unwrap();
    assert_eq!(agreement.state, ClientAgreementState::Rejected);

    let events = req_market
        .query_agreement_events(&None, 0.0, Some(3), ref_timestamp, &req_id)
        .await
        .unwrap();

    assert_eq!(events.len(), 1);
    match &events[0].event_type {
        AgreementEventType::AgreementRejectedEvent { reason } => {
            assert_eq!(reason.as_ref().unwrap().message, "Not-interested");
        }
        e => panic!(
            "Expected AgreementEventType::AgreementRejectedEvent, got: {:?}",
            e
        ),
    };

    let events = prov_market
        .query_agreement_events(&None, 0.0, Some(3), ref_timestamp, &prov_id)
        .await
        .unwrap();

    assert_eq!(events.len(), 1);
    match &events[0].event_type {
        AgreementEventType::AgreementRejectedEvent { reason } => {
            assert_eq!(reason.as_ref().unwrap().message, "Not-interested");
        }
        e => panic!(
            "Expected AgreementEventType::AgreementRejectedEvent, got: {:?}",
            e
        ),
    };
}
