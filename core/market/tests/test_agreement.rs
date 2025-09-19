use actix_web::{http::StatusCode, web::Bytes};
use chrono::{Duration, Utc};
use ya_framework_basic::log::enable_logs;

use ya_client::model::market::Role;
use ya_core_model::market;

use ya_framework_mocks::assert_err_eq;
use ya_framework_mocks::market::legacy::agreement_utils::{gen_reason, negotiate_agreement};
use ya_framework_mocks::market::legacy::events_helper::*;
use ya_framework_mocks::market::legacy::mock_agreement::generate_agreement;
use ya_framework_mocks::market::legacy::mock_node::MarketsNetwork;
use ya_framework_mocks::market::legacy::proposal_util::{
    exchange_draft_proposals, NegotiationHelper,
};
use ya_framework_mocks::net::MockNet;

use ya_framework_basic::temp_dir;
use ya_market::testing::{
    mock_offer::client::{sample_demand, sample_offer},
    AgreementDao, AgreementDaoError, AgreementError, AgreementState, ApprovalStatus,
    MarketServiceExt, Owner, ProposalState, WaitForApprovalError,
};
use ya_service_bus::timeout::IntoTimeoutFuture;
use ya_service_bus::RpcEndpoint;

const REQ_NAME: &str = "Node-1";
const PROV_NAME: &str = "Node-2";

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_gsb_get_agreement() -> anyhow::Result<()> {
    enable_logs(false);
    let dir = temp_dir!("test_gsb_get_agreement")?;
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
    let prov_id = network.get_default_id(PROV_NAME).await;

    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await
        .unwrap();

    // than: confirm agreement with app_session_id
    let sess_id = Some("sess-ajdi".into());
    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, sess_id.clone())
        .await
        .unwrap();

    let agreement = network
        .market_gsb_prefixes(REQ_NAME)
        .local()
        .send(market::GetAgreement {
            agreement_id: agreement_id.into_client(),
            role: Role::Requestor,
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(agreement.agreement_id, agreement_id.into_client());
    assert_eq!(agreement.demand.requestor_id, req_id.identity);
    assert_eq!(agreement.offer.provider_id, prov_id.identity);
    assert_eq!(agreement.app_session_id, sess_id);

    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_gsb_list_agreements() -> anyhow::Result<()> {
    enable_logs(false);
    let dir = temp_dir!("test_gsb_list_agreements")?;
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

    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await
        .unwrap();

    // than: confirm agreement with app_session_id
    let sess_id = Some("sess-iksde".into());
    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, sess_id.clone())
        .await
        .unwrap();

    let agreements = network
        .market_gsb_prefixes(REQ_NAME)
        .local()
        .send(market::ListAgreements::default())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(agreements.len(), 1);
    assert_eq!(agreements[0].id, agreement_id.into_client());
    assert_eq!(agreements[0].role, Role::Requestor);

    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_get_agreement() -> anyhow::Result<()> {
    enable_logs(false);
    let dir = temp_dir!("test_get_agreement")?;
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
    let prov_id = network.get_default_id(PROV_NAME).await;

    let agreement_id = req_engine
        .create_agreement(req_id.clone(), &proposal_id, Utc::now())
        .await
        .unwrap();

    let agreement = req_market
        .get_agreement(&agreement_id, &req_id)
        .await
        .unwrap();
    assert_eq!(agreement.agreement_id, agreement_id.into_client());
    assert_eq!(agreement.demand.requestor_id, req_id.identity);
    assert_eq!(agreement.offer.provider_id, prov_id.identity);

    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_rest_get_not_existing_agreement() -> anyhow::Result<()> {
    enable_logs(false);
    let dir = temp_dir!("test_rest_get_not_existing_agreement")?;
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

    // Create invalid id. Translation to provider id should give us
    // something, that can't be found on Requestor.
    let agreement_id = req_engine
        .create_agreement(req_id.clone(), &proposal_id, Utc::now())
        .await
        .unwrap()
        .translate(Owner::Provider);

    let result = req_market.get_agreement(&agreement_id, &req_id).await;
    assert!(result.is_err());
    assert_err_eq!(
        AgreementError::NotFound(agreement_id.to_string()).to_string(),
        result
    );
    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn full_market_interaction_aka_happy_path() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("full_market_interaction_aka_happy_path")?;
    let dir = dir.path();

    let network = MarketsNetwork::new_containerized(dir, MockNet::new())
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

    assert_eq!(
        req_market
            .get_proposal_from_db(&proposal_id)
            .await
            .unwrap()
            .body
            .state,
        ProposalState::Accepted
    );

    // Confirms it immediately
    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await
        .unwrap();

    // And starts waiting for Agreement approval by Provider
    let agr_id = agreement_id.clone();
    let query_handle = tokio::spawn(async move {
        let approval_status = req_market
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

    Ok(())
}

/// Requestor can't counter the same Proposal for the second time.
// TODO: Should it be allowed after expiration.unwrap().unwrap() For sure it shouldn't be allowed
// TODO: after rejection, because rejection always ends negotiations.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn second_creation_should_fail() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("second_creation_should_fail")?;
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

    // when: expiration time is now
    let _agreement_id = req_engine
        .create_agreement(req_id.clone(), &proposal_id, Utc::now())
        .await
        .unwrap();

    let result = req_engine
        .create_agreement(req_id.clone(), &proposal_id, Utc::now())
        .await;

    assert_err_eq!(AgreementError::ProposalAlreadyAccepted(proposal_id), result);

    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn second_confirmation_should_fail() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("second_confirmation_should_fail")?;
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

    // when: expiration time is now
    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await
        .unwrap();

    // than: first try to confirm agreement should pass
    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await
        .unwrap();

    // but second should fail
    let result = req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await;
    assert_err_eq!(
        AgreementError::UpdateState(
            agreement_id,
            AgreementDaoError::InvalidTransition {
                from: AgreementState::Pending,
                to: AgreementState::Pending
            }
        ),
        result,
    );

    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn agreement_expired_before_confirmation() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("agreement_expired_before_confirmation")?;
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

    // when: expiration time is now
    let agreement_id = req_engine
        .create_agreement(req_id.clone(), &proposal_id, Utc::now())
        .await
        .unwrap();

    // try to wait a bit, because CI on Windows is failing here...
    tokio::time::sleep(Duration::milliseconds(50).to_std().unwrap()).await;

    // than: a try to confirm agreement...
    let result = req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await;

    // results with Expired error
    assert_err_eq!(
        AgreementError::UpdateState(
            agreement_id,
            AgreementDaoError::InvalidTransition {
                from: AgreementState::Expired,
                to: AgreementState::Pending
            }
        ),
        result,
    );

    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn agreement_expired_before_approval() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("agreement_expired_before_approval")?;
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

    // when: expiration time is now
    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::milliseconds(30),
        )
        .await
        .unwrap();

    // than: immediate confirm agreement should pass
    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await
        .unwrap();

    tokio::time::sleep(Duration::milliseconds(50).to_std().unwrap()).await;

    // waiting for approval results with Expired error
    // bc Provider does not approve the Agreement
    let result = req_engine
        .wait_for_approval(&agreement_id, 0.1)
        .timeout(Some(std::time::Duration::from_millis(500)))
        .await
        .expect("`wait_for_approval` should timeout internally");

    assert_err_eq!(WaitForApprovalError::Expired(agreement_id), result);

    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn waiting_wo_confirmation_should_fail() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("waiting_wo_confirmation_should_fail")?;
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

    // when: expiration time is now
    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await
        .unwrap();

    // waiting for approval results with not confirmed error
    let result = req_engine.wait_for_approval(&agreement_id, 0.1).await;
    assert_err_eq!(WaitForApprovalError::NotConfirmed(agreement_id), result);

    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn approval_before_confirmation_should_fail() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("approval_before_confirmation_should_fail")?;
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
    let prov_id = network.get_default_id(PROV_NAME).await;

    // Requestor creates agreement with 1h expiration
    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await
        .unwrap();

    // Provider tries to approve the Agreement
    let result = network
        .get_market(PROV_NAME)
        .provider_engine
        .approve_agreement(prov_id.clone(), &agreement_id, None, 0.1)
        .await;

    // ... which results in not found error, bc there was no confirmation
    // so Requestor did not send an Agreement
    assert_err_eq!(AgreementError::NotFound(agreement_id.to_string()), result);

    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn approval_without_waiting_should_pass() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("approval_without_waiting_should_pass")?;
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
    let prov_id = network.get_default_id(PROV_NAME).await;

    // Requestor creates agreement with 1h expiration
    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await
        .unwrap();

    // Confirms it immediately
    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await
        .unwrap();

    // Provider successfully approves the Agreement
    // even though Requestor does not wait for it
    network
        .get_market(PROV_NAME)
        .provider_engine
        .approve_agreement(
            prov_id.clone(),
            &agreement_id.translate(Owner::Provider),
            None,
            0.1,
        )
        .await
        .unwrap();

    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn waiting_after_approval_should_pass() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("waiting_after_approval_should_pass")?;
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
    let prov_id = network.get_default_id(PROV_NAME).await;

    // Requestor creates agreement with 1h expiration
    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await
        .unwrap();

    // Confirms it immediately
    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await
        .unwrap();

    // Provider successfully approves the Agreement
    network
        .get_market(PROV_NAME)
        .provider_engine
        .approve_agreement(
            prov_id.clone(),
            &agreement_id.clone().translate(Owner::Provider),
            None,
            0.1,
        )
        .await
        .unwrap();

    // Requestor successfully waits for the Agreement approval
    let approval_status = req_engine
        .wait_for_approval(&agreement_id, 0.1)
        .await
        .unwrap();
    assert_eq!(approval_status, ApprovalStatus::Approved);

    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn second_approval_should_fail() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("second_approval_should_fail")?;
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
    let prov_id = network.get_default_id(PROV_NAME).await;

    // Requestor creates agreement with 1h expiration
    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await
        .unwrap();

    // Confirms it immediately
    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await
        .unwrap();

    // Provider successfully approves the Agreement
    // even though Requestor does not wait for it
    let prov_market = &network.get_market(PROV_NAME).provider_engine;

    // First approval succeeds
    prov_market
        .approve_agreement(
            prov_id.clone(),
            &agreement_id.clone().translate(Owner::Provider),
            None,
            0.1,
        )
        .await
        .unwrap();

    // ... but second fails
    let result = prov_market
        .approve_agreement(
            prov_id.clone(),
            &agreement_id.clone().translate(Owner::Provider),
            None,
            0.1,
        )
        .await;
    let agreement_id = agreement_id.clone().translate(Owner::Provider);
    assert_err_eq!(
        AgreementError::UpdateState(
            agreement_id,
            AgreementDaoError::InvalidTransition {
                from: AgreementState::Approved,
                to: AgreementState::Approving
            }
        ),
        result
    );

    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn second_waiting_should_pass() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("second_waiting_should_pass")?;
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

    // Confirms it immediately
    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await
        .unwrap();

    // Provider successfully approves the Agreement
    let prov_id = network.get_default_id(PROV_NAME).await;
    network
        .get_market(PROV_NAME)
        .provider_engine
        .approve_agreement(
            prov_id.clone(),
            &agreement_id.clone().translate(Owner::Provider),
            None,
            0.1,
        )
        .await
        .unwrap();

    // Requestor successfully waits for the Agreement approval first time
    let approval_status = req_engine
        .wait_for_approval(&agreement_id, 0.1)
        .timeout(Some(std::time::Duration::from_millis(500)))
        .await
        .expect("`wait_for_approval` shoudl timeout internally")
        .unwrap();
    assert_eq!(approval_status, ApprovalStatus::Approved);

    // second wait should also succeed
    let approval_status = req_engine
        .wait_for_approval(&agreement_id, 0.1)
        .timeout(Some(std::time::Duration::from_millis(500)))
        .await
        .expect("`wait_for_approval` shoudl timeout internally")
        .unwrap();
    assert_eq!(approval_status, ApprovalStatus::Approved);

    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn net_err_while_confirming() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("net_err_while_confirming")?;
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

    // when
    network.break_networking_for(PROV_NAME).await.unwrap();

    // then confirm should
    let result = req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await;
    match result.unwrap_err() {
        AgreementError::ProtocolCreate(_) => (),
        e => panic!("expected protocol error, but got: {}", e),
    };
    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn net_err_while_approving() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("net_err_while_approving")?;
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

    // Confirms it immediately
    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await
        .unwrap();

    // when
    network.break_networking_for(REQ_NAME).await.unwrap();

    // then approve should fail
    let prov_id = network.get_default_id(PROV_NAME).await;
    let result = network
        .get_market(PROV_NAME)
        .provider_engine
        .approve_agreement(
            prov_id.clone(),
            &agreement_id.clone().translate(Owner::Provider),
            None,
            0.1,
        )
        .await;

    match result.unwrap_err() {
        AgreementError::Protocol(_) => (),
        e => panic!("expected protocol error, but got: {}", e),
    };

    Ok(())
}

/// Requestor can create Agreements only from Proposals, that came from Provider.
/// He can turn his own Proposal into Agreement.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn cant_promote_requestor_proposal() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("cant_promote_requestor_proposal")?;
    let dir = dir.path();
    let network = MarketsNetwork::new_mocked(dir, MockNet::new())
        .await?
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;

    let NegotiationHelper {
        proposal_id,
        demand_id,
        ..
    } = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME)
        .await
        .unwrap();

    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME).await;

    let our_proposal = sample_demand();
    let our_proposal_id = req_market
        .requestor_engine
        .counter_proposal(&demand_id, &proposal_id, &our_proposal, &req_id)
        .await
        .unwrap();

    // Requestor tries to promote his own Proposal to Agreement.
    match req_engine
        .create_agreement(
            req_id.clone(),
            &our_proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await
    {
        Err(AgreementError::OwnProposal(id)) => assert_eq!(id, our_proposal_id),
        e => panic!("Expected AgreementError::OwnProposal, got: {:?}", e),
    }

    Ok(())
}

/// Requestor can't create Agreement from initial Proposal. At least one step
/// of negotiations must happen, before he can create Agreement.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn cant_promote_initial_proposal() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("cant_promote_initial_proposal")?;
    let dir = dir.path();
    let network = MarketsNetwork::new_mocked(dir, MockNet::new())
        .await?
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;

    let req_market = network.get_market(REQ_NAME);
    let req_identity = network.get_default_id(REQ_NAME).await;
    let prov_market = network.get_market(PROV_NAME);
    let prov_identity = network.get_default_id(PROV_NAME).await;

    let demand_id = req_market
        .subscribe_demand(&sample_demand(), &req_identity)
        .await
        .unwrap();
    prov_market
        .subscribe_offer(&sample_offer(), &prov_identity)
        .await
        .unwrap();

    let proposal = requestor::query_proposal(&req_market, &demand_id, "Requestor query_events")
        .await
        .unwrap();
    let proposal_id = proposal.get_proposal_id().unwrap();

    match req_market
        .requestor_engine
        .create_agreement(
            req_identity.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await
    {
        Err(AgreementError::NoNegotiations(id)) => assert_eq!(id, proposal_id),
        e => panic!("Expected AgreementError::NoNegotiations, got: {:?}", e),
    }

    Ok(())
}

/// Requestor can promote only last proposal in negotiation chain.
/// If negotiations were more advanced, `create_agreement` will end with error.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn cant_promote_not_last_proposal() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("cant_promote_not_last_proposal")?;
    let dir = dir.path();
    let network = MarketsNetwork::new_mocked(dir, MockNet::new())
        .await?
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;

    let NegotiationHelper {
        proposal_id,
        demand_id,
        ..
    } = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME)
        .await
        .unwrap();

    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME).await;

    let our_proposal = sample_demand();
    req_market
        .requestor_engine
        .counter_proposal(&demand_id, &proposal_id, &our_proposal, &req_id)
        .await
        .unwrap();

    // Requestor tries to promote Proposal that was already followed by
    // further negotiations.
    match req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await
    {
        Err(AgreementError::ProposalCountered(id)) => assert_eq!(id, proposal_id),
        e => panic!("Expected AgreementError::ProposalCountered, got: {:?}", e),
    }

    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_terminate() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("test_terminate")?;
    let dir = dir.path();

    let network = MarketsNetwork::new_mocked(dir, MockNet::new())
        .await?
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;
    let req_market = network.get_market(REQ_NAME);
    let prov_market = network.get_market(PROV_NAME);
    let req_identity = network.get_default_id(REQ_NAME).await;
    let req_agreement_dao = req_market.db.as_dao::<AgreementDao>();
    let prov_agreement_dao = prov_market.db.as_dao::<AgreementDao>();
    let agreement_1 = generate_agreement(1, (Utc::now() + Duration::days(1)).naive_utc());
    req_agreement_dao.save(agreement_1.clone()).await.unwrap();
    prov_agreement_dao.save(agreement_1.clone()).await.unwrap();

    let reason =
        serde_json::from_value(serde_json::json!({"ala":"ma kota","message": "coś"})).unwrap();
    req_market
        .terminate_agreement(req_identity, agreement_1.id.into_client(), Some(reason))
        .await
        .ok();

    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_terminate_not_existing_agreement() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("test_terminate_not_existing_agreement")?;
    let dir = dir.path();

    let network = MarketsNetwork::new_mocked(dir, MockNet::new())
        .await?
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;

    let req_market = network.get_market(REQ_NAME);
    let req_id = network.get_default_id(REQ_NAME).await;

    negotiate_agreement(
        &network,
        REQ_NAME,
        PROV_NAME,
        "negotiation",
        "r-session",
        "p-session",
    )
    .await
    .unwrap();

    let not_existing_agreement = generate_agreement(1, Utc::now().naive_utc())
        .id
        .into_client();

    let result = req_market
        .terminate_agreement(
            req_id,
            not_existing_agreement.clone(),
            Some(gen_reason("Success")),
        )
        .await;

    match result.unwrap_err() {
        AgreementError::NotFound(id) => assert_eq!(not_existing_agreement, id),
        e => panic!("Expected AgreementError::NotFound, got: {}", e),
    };

    Ok(())
}

/// Terminate is allowed only in `Approved` state of Agreement.
/// TODO: Add tests for terminate_agreement in Cancelled and Rejected state after
///  endpoints will be implemented.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_terminate_from_wrong_states() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("test_terminate_from_wrong_states")?;
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
    let req_id = network.get_default_id(REQ_NAME).await;
    let prov_market = network.get_market(PROV_NAME);

    let agreement_id = req_market
        .requestor_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await
        .unwrap();

    let result = req_market
        .terminate_agreement(
            req_id.clone(),
            agreement_id.into_client(),
            Some(gen_reason("Failure")),
        )
        .await;

    match result {
        Ok(_) => panic!("Terminate Agreement should fail."),
        Err(AgreementError::UpdateState(
            id,
            AgreementDaoError::InvalidTransition {
                from: AgreementState::Proposal,
                to: AgreementState::Terminated,
            },
        )) => assert_eq!(id, agreement_id),
        e => panic!("Wrong error returned, got: {:?}", e),
    };

    req_market
        .requestor_engine
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await
        .unwrap();

    // Terminate can be done on both sides at this moment.
    let result = req_market
        .terminate_agreement(
            req_id.clone(),
            agreement_id.into_client(),
            Some(gen_reason("Failure")),
        )
        .await;

    match result {
        Ok(_) => panic!("Terminate Agreement should fail."),
        Err(AgreementError::UpdateState(
            id,
            AgreementDaoError::InvalidTransition {
                from: AgreementState::Pending,
                to: AgreementState::Terminated,
            },
        )) => assert_eq!(id, agreement_id),
        e => panic!("Wrong error returned, got: {:?}", e),
    };

    let agreement_id = agreement_id.clone().translate(Owner::Provider);

    let result = prov_market
        .terminate_agreement(
            req_id,
            agreement_id.into_client(),
            Some(gen_reason("Failure")),
        )
        .await;

    match result {
        Ok(_) => panic!("Terminate Agreement should fail."),
        Err(AgreementError::UpdateState(
            id,
            AgreementDaoError::InvalidTransition {
                from: AgreementState::Pending,
                to: AgreementState::Terminated,
            },
        )) => assert_eq!(id, agreement_id),
        e => panic!("Wrong error returned, got: {:?}", e),
    };
    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_terminate_rejected_agreement() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("test_terminate_rejected_agreement")?;
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

    let prov_market = network.get_market(PROV_NAME);
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME).await;
    let prov_id = network.get_default_id(PROV_NAME).await;

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

    let result = req_market
        .terminate_agreement(
            req_id.clone(),
            agreement_id.into_client(),
            Some(gen_reason("Failure")),
        )
        .await;

    match result {
        Ok(_) => panic!("Terminate Agreement should fail."),
        Err(AgreementError::UpdateState(
            id,
            AgreementDaoError::InvalidTransition {
                from: AgreementState::Rejected,
                to: AgreementState::Terminated,
            },
        )) => assert_eq!(id, agreement_id),
        e => panic!("Wrong error returned, got: {:?}", e),
    };

    Ok(())
}

/// We expect, that reason string is structured and can\
/// be deserialized to `Reason` struct.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_terminate_invalid_reason() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("test_terminate_invalid_reason")?;
    let dir = dir.path();
    let network = MarketsNetwork::new_mocked(dir, MockNet::new())
        .await?
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;

    let agreement_id = negotiate_agreement(
        &network,
        REQ_NAME,
        PROV_NAME,
        "negotiation",
        "r-session",
        "p-session",
    )
    .await
    .unwrap()
    .r_agreement;

    let app = network.get_rest_app(REQ_NAME).await;
    let url = format!(
        "/market-api/v1/agreements/{}/terminate",
        agreement_id.into_client(),
    );

    let reason = "Unstructured message. Should be json.".to_string();
    let req = actix_web::test::TestRequest::post()
        .uri(&url)
        .set_payload(Bytes::copy_from_slice(reason.as_bytes()))
        .to_request();

    let resp = actix_web::test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let reason = "{'no_message_field': 'Reason expects message field'}".to_string();
    let req = actix_web::test::TestRequest::post()
        .uri(&url)
        .set_payload(Bytes::copy_from_slice(reason.as_bytes()))
        .to_request();

    let resp = actix_web::test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    Ok(())
}
