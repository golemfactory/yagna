use anyhow::Result;
use chrono::{Duration, Utc};

use ya_core_model::market;
use ya_market_decentralized::testing::proposal_util::exchange_draft_proposals;
use ya_market_decentralized::testing::MarketsNetwork;
use ya_market_decentralized::testing::{
    AgreementError, ApprovalStatus, OwnerType, WaitForApprovalError,
};
use ya_service_bus::typed as bus;
use ya_service_bus::RpcEndpoint;

const REQ_NAME: &str = "Node-1";
const PROV_NAME: &str = "Node-2";

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_gsb_get_agreement() -> Result<()> {
    let network = MarketsNetwork::new("test_gsb_get_agreement")
        .await
        .add_market_instance(REQ_NAME)
        .await?
        .add_market_instance(PROV_NAME)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME).await?;
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);
    let prov_id = network.get_default_id(PROV_NAME);

    let agreement_id = req_engine
        .create_agreement(req_id.clone(), &proposal_id, Utc::now())
        .await?;
    let agreement = bus::service(network.node_gsb_prefixes(REQ_NAME).0)
        .send(market::GetAgreement {
            agreement_id: agreement_id.to_string(),
        })
        .await??;
    assert_eq!(agreement.agreement_id, agreement_id.to_string());
    assert_eq!(
        agreement.demand.requestor_id.unwrap(),
        req_id.identity.to_string()
    );
    assert_eq!(
        agreement.offer.provider_id.unwrap(),
        prov_id.identity.to_string()
    );
    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_rest_get_agreement() -> Result<()> {
    let network = MarketsNetwork::new("test_rest_get_agreement")
        .await
        .add_market_instance(REQ_NAME)
        .await?
        .add_market_instance(PROV_NAME)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME).await?;
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);
    let prov_id = network.get_default_id(PROV_NAME);

    let agreement_id = req_engine
        .create_agreement(req_id.clone(), &proposal_id, Utc::now())
        .await?;

    let agreement = req_market.get_agreement(&agreement_id, &req_id).await?;
    assert_eq!(agreement.agreement_id, agreement_id.to_string());
    assert_eq!(
        agreement.demand.requestor_id.unwrap(),
        req_id.identity.to_string()
    );
    assert_eq!(
        agreement.offer.provider_id.unwrap(),
        prov_id.identity.to_string()
    );
    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_rest_get_not_existing_agreement() -> Result<()> {
    let network = MarketsNetwork::new("test_rest_get_not_existing_agreement")
        .await
        .add_market_instance(REQ_NAME)
        .await?
        .add_market_instance(PROV_NAME)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME).await?;
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);

    // Create invalid id. Translation to provider id should give us
    // something, that can't be found on Requestor.
    let agreement_id = req_engine
        .create_agreement(req_id.clone(), &proposal_id, Utc::now())
        .await?
        .translate(OwnerType::Provider);

    let result = req_market.get_agreement(&agreement_id, &req_id).await;
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().to_string(),
        AgreementError::NotFound(agreement_id.clone()).to_string()
    );
    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn full_market_interaction_aka_happy_path() -> Result<()> {
    let network = MarketsNetwork::new("full_market_interaction_aka_happy_path")
        .await
        .add_market_instance(REQ_NAME)
        .await?
        .add_market_instance(PROV_NAME)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME).await?;
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);

    // Requestor creates agreement with 1h expiration
    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await?;

    // Confirms it immediately
    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id)
        .await?;

    // And starts waiting for Agreement approval by Provider
    let agr_id = agreement_id.clone();
    let query_handle = tokio::spawn(async move {
        let approval_status = req_market
            .requestor_engine
            .wait_for_approval(&agr_id, 0.1)
            .await?;

        assert_eq!(
            approval_status.to_string(),
            ApprovalStatus::Approved.to_string()
        );
        Result::<(), anyhow::Error>::Ok(())
    });

    // Provider approves the Agreement and waits for ack
    network
        .get_market(PROV_NAME)
        .provider_engine
        .approve_agreement(network.get_default_id(PROV_NAME), &agreement_id, 0.1)
        .await?;

    // Protect from eternal waiting.
    tokio::time::timeout(Duration::milliseconds(150).to_std()?, query_handle).await???;

    Ok(())
}

// TODO: It is allowed in general, but probably after rejection or expiration??
// TODO: but we don't know even how we should handle this case
//#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[ignore]
#[actix_rt::test]
async fn second_creation_should_fail() -> Result<()> {
    let network = MarketsNetwork::new("second_creation_should_fail")
        .await
        .add_market_instance(REQ_NAME)
        .await?
        .add_market_instance(PROV_NAME)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME).await?;
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);

    // when: expiration time is now
    let agreement_id = req_engine
        .create_agreement(req_id.clone(), &proposal_id, Utc::now())
        .await?;

    let result = req_engine
        .create_agreement(req_id.clone(), &proposal_id, Utc::now())
        .await;

    assert_eq!(
        result.unwrap_err().to_string(),
        AgreementError::Confirmed(agreement_id).to_string()
    );

    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn second_confirmation_should_fail() -> Result<()> {
    let network = MarketsNetwork::new("second_confirmation_should_fail")
        .await
        .add_market_instance(REQ_NAME)
        .await?
        .add_market_instance(PROV_NAME)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME).await?;
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);

    // when: expiration time is now
    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await?;

    // than: first try to confirm agreement should pass
    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id)
        .await?;

    // but second should fail
    let result = req_engine
        .confirm_agreement(req_id.clone(), &agreement_id)
        .await;
    assert_eq!(
        result.unwrap_err().to_string(),
        AgreementError::Confirmed(agreement_id).to_string()
    );

    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn agreement_expired_before_confirmation() -> Result<()> {
    let network = MarketsNetwork::new("agreement_expired_before_confirmation")
        .await
        .add_market_instance(REQ_NAME)
        .await?
        .add_market_instance(PROV_NAME)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME).await?;
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);

    // when: expiration time is now
    let agreement_id = req_engine
        .create_agreement(req_id.clone(), &proposal_id, Utc::now())
        .await?;

    // than: a try to confirm agreement...
    let result = req_engine
        .confirm_agreement(req_id.clone(), &agreement_id)
        .await;

    // results with Expired error
    assert_eq!(
        result.unwrap_err().to_string(),
        AgreementError::Expired(agreement_id).to_string()
    );

    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn agreement_expired_before_approval() -> Result<()> {
    let network = MarketsNetwork::new("agreement_expired_before_approval")
        .await
        .add_market_instance(REQ_NAME)
        .await?
        .add_market_instance(PROV_NAME)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME).await?;
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);

    // when: expiration time is now
    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::milliseconds(30),
        )
        .await?;

    // than: immediate confirm agreement should pass
    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id)
        .await?;

    tokio::time::delay_for(Duration::milliseconds(50).to_std()?).await;

    // waiting for approval results with Expired error
    // bc Provider does not approve the Agreement
    let result = req_engine.wait_for_approval(&agreement_id, 0.1).await;

    assert_eq!(
        result.unwrap_err().to_string(),
        WaitForApprovalError::AgreementExpired(agreement_id).to_string()
    );

    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn waiting_wo_confirmation_should_fail() -> Result<()> {
    let network = MarketsNetwork::new("waiting_wo_confirmation_should_fail")
        .await
        .add_market_instance(REQ_NAME)
        .await?
        .add_market_instance(PROV_NAME)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME).await?;
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);

    // when: expiration time is now
    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await?;

    // waiting for approval results with not confirmed error
    let result = req_engine.wait_for_approval(&agreement_id, 0.1).await;

    assert_eq!(
        result.unwrap_err().to_string(),
        WaitForApprovalError::AgreementNotConfirmed(agreement_id).to_string()
    );

    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn approval_before_confirmation_should_fail() -> Result<()> {
    let network = MarketsNetwork::new("approval_before_confirmation_should_fail")
        .await
        .add_market_instance(REQ_NAME)
        .await?
        .add_market_instance(PROV_NAME)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME).await?;
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);
    let prov_id = network.get_default_id(PROV_NAME);

    // Requestor creates agreement with 1h expiration
    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await?;

    // Provider tries to approve the Agreement
    let result = network
        .get_market(PROV_NAME)
        .provider_engine
        .approve_agreement(prov_id.clone(), &agreement_id, 0.1)
        .await;

    // ... which results in not found error, bc there was no confirmation
    // so Requestor did not send an Agreement
    assert_eq!(
        result.unwrap_err().to_string(),
        AgreementError::NotFound(agreement_id).to_string()
    );

    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn approval_without_waiting_should_pass() -> Result<()> {
    let network = MarketsNetwork::new("approval_without_waiting_should_pass")
        .await
        .add_market_instance(REQ_NAME)
        .await?
        .add_market_instance(PROV_NAME)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME).await?;
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);
    let prov_id = network.get_default_id(PROV_NAME);

    // Requestor creates agreement with 1h expiration
    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await?;

    // Confirms it immediately
    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id)
        .await?;

    // Provider successfully approves the Agreement
    // even though Requestor does not wait for it
    network
        .get_market(PROV_NAME)
        .provider_engine
        .approve_agreement(prov_id.clone(), &agreement_id, 0.1)
        .await?;

    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn waiting_after_approval_should_pass() -> Result<()> {
    let network = MarketsNetwork::new("waiting_after_approval_should_pass")
        .await
        .add_market_instance(REQ_NAME)
        .await?
        .add_market_instance(PROV_NAME)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME).await?;
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);
    let prov_id = network.get_default_id(PROV_NAME);

    // Requestor creates agreement with 1h expiration
    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await?;

    // Confirms it immediately
    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id)
        .await?;

    // Provider successfully approves the Agreement
    network
        .get_market(PROV_NAME)
        .provider_engine
        .approve_agreement(prov_id.clone(), &agreement_id, 0.1)
        .await?;

    // Requestor successfully waits for the Agreement approval
    let approval_status = req_engine.wait_for_approval(&agreement_id, 0.1).await?;
    assert_eq!(
        approval_status.to_string(),
        ApprovalStatus::Approved.to_string()
    );

    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn second_approval_should_fail() -> Result<()> {
    let network = MarketsNetwork::new("second_approval_should_fail")
        .await
        .add_market_instance(REQ_NAME)
        .await?
        .add_market_instance(PROV_NAME)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME).await?;
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);
    let prov_id = network.get_default_id(PROV_NAME);

    // Requestor creates agreement with 1h expiration
    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await?;

    // Confirms it immediately
    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id)
        .await?;

    // Provider successfully approves the Agreement
    // even though Requestor does not wait for it
    let prov_market = &network.get_market(PROV_NAME).provider_engine;

    // First approval succeeds
    prov_market
        .approve_agreement(prov_id.clone(), &agreement_id, 0.1)
        .await?;

    // ... but second fails
    let result = prov_market
        .approve_agreement(prov_id.clone(), &agreement_id, 0.1)
        .await;
    assert_eq!(
        result.unwrap_err().to_string(),
        AgreementError::Approved(agreement_id).to_string()
    );

    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn second_waiting_should_pass() -> Result<()> {
    let network = MarketsNetwork::new("second_waiting_should_pass")
        .await
        .add_market_instance(REQ_NAME)
        .await?
        .add_market_instance(PROV_NAME)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME).await?;
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);

    // Requestor creates agreement with 1h expiration
    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await?;

    // Confirms it immediately
    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id)
        .await?;

    // Provider successfully approves the Agreement
    let prov_id = network.get_default_id(PROV_NAME);
    network
        .get_market(PROV_NAME)
        .provider_engine
        .approve_agreement(prov_id.clone(), &agreement_id, 0.1)
        .await?;

    // Requestor successfully waits for the Agreement approval first time
    let approval_status = req_engine.wait_for_approval(&agreement_id, 0.1).await?;
    assert_eq!(
        approval_status.to_string(),
        ApprovalStatus::Approved.to_string()
    );

    // second wait should also succeed
    let approval_status = req_engine.wait_for_approval(&agreement_id, 0.1).await?;
    assert_eq!(
        approval_status.to_string(),
        ApprovalStatus::Approved.to_string()
    );

    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn net_err_while_confirming() -> Result<()> {
    let network = MarketsNetwork::new("net_err_while_confirming")
        .await
        .add_market_instance(REQ_NAME)
        .await?
        .add_market_instance(PROV_NAME)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME).await?;
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);

    // Requestor creates agreement with 1h expiration
    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await?;

    // when
    network.break_networking_for(PROV_NAME)?;

    // then confirm should
    let result = req_engine
        .confirm_agreement(req_id.clone(), &agreement_id)
        .await;
    match result.unwrap_err() {
        AgreementError::Protocol(_) => (),
        e => panic!("expected protocol error, but got: {}", e),
    };

    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn net_err_while_approving() -> Result<()> {
    let network = MarketsNetwork::new("net_err_while_approving")
        .await
        .add_market_instance(REQ_NAME)
        .await?
        .add_market_instance(PROV_NAME)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, REQ_NAME, PROV_NAME).await?;
    let req_market = network.get_market(REQ_NAME);
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id(REQ_NAME);

    // Requestor creates agreement with 1h expiration
    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await?;

    // Confirms it immediately
    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id)
        .await?;

    // when
    network.break_networking_for(REQ_NAME)?;

    // then approve should fail
    let prov_id = network.get_default_id(PROV_NAME);
    let result = network
        .get_market(PROV_NAME)
        .provider_engine
        .approve_agreement(prov_id.clone(), &agreement_id, 0.1)
        .await;

    match result.unwrap_err() {
        AgreementError::ProtocolApprove(_) => (),
        e => panic!("expected protocol error, but got: {}", e),
    };

    Ok(())
}
