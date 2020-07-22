use chrono::{Duration, Utc};

use ya_core_model::market;
use ya_market_decentralized::testing::proposal_util::exchange_draft_proposals;
use ya_market_decentralized::testing::MarketsNetwork;
use ya_market_decentralized::testing::{AgreementError, ApprovalStatus, WaitForApprovalError};
use ya_service_bus::typed as bus;
use ya_service_bus::RpcEndpoint;

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_gsb_get_agreement() -> anyhow::Result<()> {
    let node_id1 = "Node-1";
    let node_id2 = "Node-2";
    let network = MarketsNetwork::new("test_gsb_get_agreement")
        .await
        .add_market_instance(node_id1)
        .await?
        .add_market_instance(node_id2)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, node_id1, node_id2).await?;
    let market = network.get_market(node_id1);
    let identity1 = network.get_default_id(node_id1);
    let identity2 = network.get_default_id(node_id2);

    let agreement_id = market
        .requestor_engine
        .create_agreement(identity1.clone(), &proposal_id, Utc::now())
        .await?;
    let agreement = bus::service(network.node_gsb_prefixes(node_id1).0)
        .send(market::GetAgreement {
            agreement_id: agreement_id.to_string(),
        })
        .await??;
    assert_eq!(agreement.agreement_id, agreement_id.to_string());
    assert_eq!(
        agreement.demand.requestor_id.unwrap(),
        identity1.identity.to_string()
    );
    assert_eq!(
        agreement.offer.provider_id.unwrap(),
        identity2.identity.to_string()
    );
    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn full_market_interaction_aka_happy_path() -> anyhow::Result<()> {
    let req = "Node-1";
    let prov = "Node-2";
    let network = MarketsNetwork::new("full_market_interaction_aka_happy_path")
        .await
        .add_market_instance(req)
        .await?
        .add_market_instance(prov)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, req, prov).await?;
    let req_market = network.get_market(req);
    let req_id = network.get_default_id(req);
    let prov_id = network.get_default_id(prov);

    // Requestor creates agreement with 1h expiration
    let agreement_id = req_market
        .requestor_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await?;

    // Confirms it immediately
    req_market
        .requestor_engine
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
        .get_market(prov)
        .provider_engine
        .approve_agreement(prov_id.clone(), &agreement_id, 0.1)
        .await?;

    // Protect from eternal waiting.
    tokio::time::timeout(Duration::milliseconds(150).to_std()?, query_handle).await???;

    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn double_confirm_agreement_should_fail() -> anyhow::Result<()> {
    let req = "Node-1";
    let prov = "Node-2";
    let network = MarketsNetwork::new("double_confirm_agreement_should_fail")
        .await
        .add_market_instance(req)
        .await?
        .add_market_instance(prov)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, req, prov).await?;
    let req_market = network.get_market(req);
    let req_id = network.get_default_id(req);

    // when: expiration time is now
    let agreement_id = req_market
        .requestor_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await?;

    // than: first try to confirm agreement should pass
    req_market
        .requestor_engine
        .confirm_agreement(req_id.clone(), &agreement_id)
        .await?;

    // but second should fail
    let result = req_market
        .requestor_engine
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
async fn agreement_expired_before_confirmation() -> anyhow::Result<()> {
    let req = "Node-1";
    let prov = "Node-2";
    let network = MarketsNetwork::new("agreement_expired_before_confirmation")
        .await
        .add_market_instance(req)
        .await?
        .add_market_instance(prov)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, req, prov).await?;
    let req_market = network.get_market(req);
    let req_id = network.get_default_id(req);

    // when: expiration time is now
    let agreement_id = req_market
        .requestor_engine
        .create_agreement(req_id.clone(), &proposal_id, Utc::now())
        .await?;

    // than: a try to confirm agreement...
    let result = req_market
        .requestor_engine
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
async fn agreement_expired_before_approval() -> anyhow::Result<()> {
    let req = "Node-1";
    let prov = "Node-2";
    let network = MarketsNetwork::new("agreement_expired_before_approval")
        .await
        .add_market_instance(req)
        .await?
        .add_market_instance(prov)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, req, prov).await?;
    let req_market = network.get_market(req);
    let req_id = network.get_default_id(req);

    // when: expiration time is now
    let agreement_id = req_market
        .requestor_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::milliseconds(30),
        )
        .await?;

    // than: immediate confirm agreement should pass
    req_market
        .requestor_engine
        .confirm_agreement(req_id.clone(), &agreement_id)
        .await?;

    tokio::time::delay_for(Duration::milliseconds(50).to_std()?).await;

    // waiting for approval results with Expired error
    // bc Provider does not approve the Agreement
    let result = req_market
        .requestor_engine
        .wait_for_approval(&agreement_id, 0.1)
        .await;

    assert_eq!(
        result.unwrap_err().to_string(),
        WaitForApprovalError::AgreementExpired(agreement_id).to_string()
    );

    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn waiting_for_not_confirmed_agreement_should_fail() -> anyhow::Result<()> {
    let req = "Node-1";
    let prov = "Node-2";
    let network = MarketsNetwork::new("waiting_for_not_confirmed_agreement_should_fail")
        .await
        .add_market_instance(req)
        .await?
        .add_market_instance(prov)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, req, prov).await?;
    let req_market = network.get_market(req);
    let req_id = network.get_default_id(req);

    // when: expiration time is now
    let agreement_id = req_market
        .requestor_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await?;

    // waiting for approval results with not confirmed error
    let result = req_market
        .requestor_engine
        .wait_for_approval(&agreement_id, 0.1)
        .await;

    assert_eq!(
        result.unwrap_err().to_string(),
        WaitForApprovalError::AgreementNotConfirmed(agreement_id).to_string()
    );

    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn approval_before_agreement_confirmation_should_fail() -> anyhow::Result<()> {
    let req = "Node-1";
    let prov = "Node-2";
    let network = MarketsNetwork::new("approval_before_agreement_confirmation_should_fail")
        .await
        .add_market_instance(req)
        .await?
        .add_market_instance(prov)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, req, prov).await?;
    let req_market = network.get_market(req);
    let req_id = network.get_default_id(req);
    let prov_id = network.get_default_id(prov);

    // Requestor creates agreement with 1h expiration
    let agreement_id = req_market
        .requestor_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await?;

    // Provider tries to approve the Agreement
    let result = network
        .get_market(prov)
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
async fn approval_without_waiting_for_agreement_should_pass() -> anyhow::Result<()> {
    let req = "Node-1";
    let prov = "Node-2";
    let network = MarketsNetwork::new("approval_without_waiting_for_agreement_should_pass")
        .await
        .add_market_instance(req)
        .await?
        .add_market_instance(prov)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, req, prov).await?;
    let req_market = network.get_market(req);
    let req_id = network.get_default_id(req);
    let prov_id = network.get_default_id(prov);

    // Requestor creates agreement with 1h expiration
    let agreement_id = req_market
        .requestor_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await?;

    // Confirms it immediately
    req_market
        .requestor_engine
        .confirm_agreement(req_id.clone(), &agreement_id)
        .await?;

    // Provider successfully approves the Agreement
    // even though Requestor does not wait for it
    network
        .get_market(prov)
        .provider_engine
        .approve_agreement(prov_id.clone(), &agreement_id, 0.1)
        .await?;

    // TODO: is it really ok to allow such situation?
    // shouldn't we return an error here?
    // assert_eq!(
    //     result.unwrap_err().to_string(),
    //     AgreementError::ProtocolApprove(..).to_string()
    // );

    Ok(())
}
