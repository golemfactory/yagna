use chrono::{Duration, Utc};

use ya_client::model::market::agreement::State as AgreementState;

use ya_market::assert_err_eq;
use ya_market::testing::{
    agreement_utils::{gen_reason, negotiate_agreement},
    events_helper::*,
    mock_node::MarketServiceExt,
    proposal_util::{exchange_draft_proposals, NegotiationHelper},
    ApprovalStatus, MarketsNetwork, Owner, WaitForApprovalError,
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
            Utc::now() + Duration::milliseconds(30),
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
    assert_eq!(agreement.state, AgreementState::Rejected);

    let agreement = prov_market
        .get_agreement(&agreement_id.clone().translate(Owner::Provider), &prov_id)
        .await
        .unwrap();
    assert_eq!(agreement.state, AgreementState::Rejected);
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
            Utc::now() + Duration::milliseconds(30),
        )
        .await
        .unwrap();

    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await
        .unwrap();

    let agr_id = agreement_id.clone().translate(Owner::Provider);
    let reject_handle = tokio::task::spawn_local(async move {
        tokio::time::delay_for(std::time::Duration::from_millis(20)).await;
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
        .wait_for_approval(&agreement_id, 0.3)
        .await
        .unwrap();
    assert_eq!(result, ApprovalStatus::Rejected);

    tokio::time::timeout(Duration::milliseconds(600).to_std().unwrap(), reject_handle)
        .await
        .unwrap()
        .unwrap();
}
