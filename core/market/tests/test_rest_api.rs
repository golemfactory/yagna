use actix_web::{body::MessageBody, dev::ServiceResponse, error::PathError, http::StatusCode};
use chrono::Utc;
use serde::de::DeserializeOwned;
use serde_json::json;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::temp_dir;

use ya_client::model::market::agreement::State as ClientAgreementState;
use ya_client::model::market::{
    agreement as client_agreement, Agreement, AgreementOperationEvent, Demand, NewDemand, NewOffer,
    Offer, Proposal, Reason,
};
use ya_client::model::ErrorMessage;
use ya_client::web::QueryParamsBuilder;
use ya_market::testing::{
    mock_offer::{
        client::{sample_demand, sample_offer},
        flatten_json,
    },
    DemandError, MarketServiceExt, ModifyOfferError, Owner, SubscriptionId, SubscriptionParseError,
};

use ya_framework_mocks::market::legacy::{
    agreement_utils::{gen_reason, negotiate_agreement},
    events_helper::requestor::expect_approve,
    mock_node::{assert_offers_broadcasted, MarketsNetwork},
    proposal_util::exchange_draft_proposals,
};
use ya_framework_mocks::net::MockNet;

const REQ_NAME: &str = "Node-1";
const PROV_NAME: &str = "Node-2";

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_rest_get_offers() -> anyhow::Result<()> {
    enable_logs(false);
    let dir = temp_dir!("test_rest_get_offers")?;
    let dir = dir.path();

    let network = MarketsNetwork::new(dir, MockNet::new())
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await;

    let market_local = network.get_market("Node-1");
    // Not really remote, but in this scenario will treat it as remote
    let market_remote = network.get_market("Node-2");
    let identity_local = network.get_default_id("Node-1").await;
    let identity_remote = network.get_default_id("Node-2").await;
    // subscribe to broadcasts
    market_remote
        .subscribe_demand(&sample_demand(), &identity_remote)
        .await
        .unwrap();

    let offer_local = NewOffer::new(json!({}), "()".to_string());
    let offer_local_unsubscribed = NewOffer::new(json!({}), "()".to_string());
    let offer_remote = NewOffer::new(json!({}), "()".to_string());
    let subscription_id_local = market_local
        .subscribe_offer(&offer_local, &identity_local)
        .await
        .unwrap();
    let subscription_id_local_unsubscribed = market_local
        .subscribe_offer(&offer_local_unsubscribed, &identity_local)
        .await
        .unwrap();
    market_local
        .unsubscribe_offer(&subscription_id_local_unsubscribed, &identity_local)
        .await
        .unwrap();
    let subscription_id_remote = market_remote
        .subscribe_offer(&offer_remote, &identity_remote)
        .await
        .unwrap();
    let offer_local = market_local
        .get_offer(&subscription_id_local)
        .await
        .unwrap();
    let _offer_remote = market_remote
        .get_offer(&subscription_id_remote)
        .await
        .unwrap();

    assert_offers_broadcasted(&[&market_remote], &[subscription_id_local.clone()]).await;

    let app = network.get_rest_app("Node-1").await;

    let req = actix_web::test::TestRequest::get()
        .uri("/market-api/v1/offers")
        .to_request();
    let resp = actix_web::test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let result: Vec<Offer> = read_response_json(resp).await;
    assert_eq!(vec![offer_local.into_client_offer().unwrap()], result);

    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_rest_get_demands() -> anyhow::Result<()> {
    enable_logs(false);
    let dir = temp_dir!("test_rest_get_demands")?;
    let dir = dir.path();

    let network = MarketsNetwork::new(dir, MockNet::new())
        .await
        .add_market_instance("Node-1")
        .await;

    let market_local = network.get_market("Node-1");
    let identity_local = network.get_default_id("Node-1").await;
    let demand_local = NewDemand::new(json!({}), "()".to_string());
    let subscription_id = market_local
        .subscribe_demand(&demand_local, &identity_local)
        .await
        .unwrap();
    let demand_local = market_local.get_demand(&subscription_id).await.unwrap();

    let app = network.get_rest_app("Node-1").await;

    let req = actix_web::test::TestRequest::get()
        .uri("/market-api/v1/demands")
        .to_request();
    let resp = actix_web::test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let result: Vec<Demand> = read_response_json(resp).await;
    assert_eq!(vec![demand_local.into_client_demand().unwrap()], result);

    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_rest_invalid_subscription_id_should_return_400() -> anyhow::Result<()> {
    enable_logs(false);
    let dir = temp_dir!("test_rest_invalid_subscription_id_should_return_400")?;
    let dir = dir.path();

    let network = MarketsNetwork::new(dir, MockNet::new())
        .await
        .add_market_instance("Node-1")
        .await;
    let app = network.get_rest_app("Node-1").await;

    let req = actix_web::test::TestRequest::delete()
        .uri("/market-api/v1/offers/invalid-id")
        .to_request();

    // when
    let resp = actix_web::test::call_service(&app, req).await;

    // then
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let result: ErrorMessage = read_response_json(resp).await;

    // let result = String::from_utf8(actix_web::test::read_body(resp).await.to_vec()).unwrap();
    assert_eq!(
        PathError::Deserialize(serde::de::Error::custom(
            SubscriptionParseError::NotHexadecimal("invalid-id".to_string())
        ))
        .to_string(),
        result.message.unwrap()
    );
    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_rest_subscribe_unsubscribe_offer() -> anyhow::Result<()> {
    enable_logs(false);
    let dir = temp_dir!("test_rest_subscribe_unsubscribe_offer")?;
    let dir = dir.path();

    let network = MarketsNetwork::new(dir, MockNet::new())
        .await
        .add_market_instance("Node-1")
        .await;
    let app = network.get_rest_app("Node-1").await;

    let client_offer = sample_offer();

    let req = actix_web::test::TestRequest::post()
        .uri("/market-api/v1/offers")
        .set_json(&client_offer)
        .to_request();

    // when create offer
    let resp = actix_web::test::call_service(&app, req).await;

    // then
    assert_eq!(resp.status(), StatusCode::CREATED);
    let subscription_id: SubscriptionId = read_response_json(resp).await;
    log::debug!("subscription_id: {}", subscription_id);

    // given
    let id = network.get_default_id("Node-1").await;
    let market = network.get_market("Node-1");
    // when get from subscription store
    let stored_offer = market.get_offer(&subscription_id).await.unwrap();
    // then
    let got_offer = stored_offer.into_client_offer().unwrap();
    assert_eq!(got_offer.offer_id, subscription_id.to_string());
    assert_eq!(got_offer.provider_id, id.identity);
    assert_eq!(&got_offer.constraints, &client_offer.constraints);
    assert_eq!(got_offer.properties, flatten_json(&client_offer.properties));

    // given
    let req = actix_web::test::TestRequest::delete()
        .uri(&format!("/market-api/v1/offers/{}", subscription_id))
        .to_request();
    // when unsubscribe
    let resp = actix_web::test::call_service(&app, req).await;
    // then
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // given
    let req = actix_web::test::TestRequest::delete()
        .uri(&format!("/market-api/v1/offers/{}", subscription_id))
        .to_request();
    // when unsubscribe again
    let resp = actix_web::test::call_service(&app, req).await;
    // then
    assert_eq!(resp.status(), StatusCode::GONE);
    let result: ErrorMessage = read_response_json(resp).await;
    // let result = String::from_utf8(actix_web::test::read_body(resp).await.to_vec()).unwrap();
    assert_eq!(
        ModifyOfferError::AlreadyUnsubscribed(subscription_id.clone()).to_string(),
        result.message.unwrap()
    );
    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_rest_subscribe_unsubscribe_demand() -> anyhow::Result<()> {
    enable_logs(false);
    let dir = temp_dir!("test_rest_subscribe_unsubscribe_demand")?;
    let dir = dir.path();

    let network = MarketsNetwork::new(dir, MockNet::new())
        .await
        .add_market_instance("Node-1")
        .await;
    let app = network.get_rest_app("Node-1").await;

    let client_demand = sample_demand();

    let req = actix_web::test::TestRequest::post()
        .uri("/market-api/v1/demands")
        .set_json(&client_demand)
        .to_request();

    // when
    let resp = actix_web::test::call_service(&app, req).await;

    // then
    assert_eq!(resp.status(), StatusCode::CREATED);
    let subscription_id: SubscriptionId = read_response_json(resp).await;
    log::debug!("subscription_id: {}", subscription_id);

    // given
    let id = network.get_default_id("Node-1").await;
    let market = network.get_market("Node-1");
    // when
    let stored_demand = market.get_demand(&subscription_id).await.unwrap();
    // then
    let got_demand = stored_demand.into_client_demand().unwrap();
    assert_eq!(got_demand.demand_id, subscription_id.to_string());
    assert_eq!(got_demand.requestor_id, id.identity);
    assert_eq!(&got_demand.constraints, &client_demand.constraints);
    assert_eq!(
        got_demand.properties,
        flatten_json(&client_demand.properties)
    );

    // given
    let req = actix_web::test::TestRequest::delete()
        .uri(&format!("/market-api/v1/demands/{}", subscription_id))
        .to_request();
    // when
    let resp = actix_web::test::call_service(&app, req).await;
    // then
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // given
    let req = actix_web::test::TestRequest::delete()
        .uri(&format!("/market-api/v1/demands/{}", subscription_id))
        .to_request();
    // when
    let resp = actix_web::test::call_service(&app, req).await;
    // then
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let result: ErrorMessage = read_response_json(resp).await;
    // let result = String::from_utf8(actix_web::test::read_body(resp).await.to_vec()).unwrap();
    assert_eq!(
        DemandError::NotFound(subscription_id.clone()).to_string(),
        result.message.unwrap()
    );
    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_rest_get_proposal() -> anyhow::Result<()> {
    enable_logs(false);
    let dir = temp_dir!("test_rest_get_proposal")?;
    let dir = dir.path();

    let network = MarketsNetwork::new(dir, MockNet::new())
        .await
        .add_market_instance("Provider")
        .await
        .add_market_instance("Requestor")
        .await;

    let prov_mkt = network.get_market("Provider");
    let proposal_id = exchange_draft_proposals(&network, "Requestor", "Provider")
        .await
        .unwrap()
        .proposal_id
        .translate(Owner::Provider);

    // Not really remote, but in this scenario will treat it as remote
    let identity_local = network.get_default_id("Provider").await;
    let offers = prov_mkt.get_offers(Some(identity_local)).await.unwrap();
    let subscription_id = &offers.first().unwrap().offer_id;
    let proposal = prov_mkt
        .get_proposal(&proposal_id)
        .await
        .unwrap()
        .into_client()
        .unwrap();
    let app = network.get_rest_app("Provider").await;

    let req_offers = actix_web::test::TestRequest::get()
        .uri(
            format!(
                "/market-api/v1/offers/{}/proposals/{}",
                subscription_id, proposal_id
            )
            .as_str(),
        )
        .to_request();
    let resp_offers = actix_web::test::call_service(&app, req_offers).await;
    assert_eq!(resp_offers.status(), StatusCode::OK);
    let result_offers: Proposal = read_response_json(resp_offers).await;
    assert_eq!(proposal, result_offers);

    let req_demands = actix_web::test::TestRequest::get()
        .uri(
            format!(
                "/market-api/v1/demands/{}/proposals/{}",
                subscription_id, proposal_id
            )
            .as_str(),
        )
        .to_request();
    let resp_demands = actix_web::test::call_service(&app, req_demands).await;
    assert_eq!(resp_demands.status(), StatusCode::OK);
    let resp_demands: Proposal = read_response_json(resp_demands).await;
    assert_eq!(proposal, resp_demands);
    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_rest_get_agreement() -> anyhow::Result<()> {
    enable_logs(false);
    let dir = temp_dir!("test_rest_get_agreement")?;
    let dir = dir.path();

    let network = MarketsNetwork::new(dir, MockNet::new())
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await;

    let proposal_id = exchange_draft_proposals(&network, "Node-1", "Node-2")
        .await
        .unwrap()
        .proposal_id;
    let req_market = network.get_market("Node-1");
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id("Node-1").await;
    let prov_id = network.get_default_id("Node-2").await;

    let agreement_id = req_engine
        .create_agreement(req_id.clone(), &proposal_id, Utc::now())
        .await
        .unwrap();

    let app = network.get_rest_app("Node-1").await;
    let req = actix_web::test::TestRequest::get()
        .uri(&format!(
            "/market-api/v1/agreements/{}",
            agreement_id.into_client()
        ))
        .to_request();
    let resp = actix_web::test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let agreement: Agreement = read_response_json(resp).await;
    assert_eq!(agreement.agreement_id, agreement_id.into_client());
    assert_eq!(agreement.demand.requestor_id, req_id.identity);
    assert_eq!(agreement.offer.provider_id, prov_id.identity);
    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_rest_query_agreement_events() -> anyhow::Result<()> {
    enable_logs(false);
    let dir = temp_dir!("test_rest_query_agreement_events")?;
    let dir = dir.path();

    let network = MarketsNetwork::new(dir, MockNet::new())
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await;

    // Will produce events to be ignored.
    let _ = negotiate_agreement(
        &network,
        "Node-1",
        "Node-2",
        "not-important",
        "not-important-session",
        "not-important-session",
    )
    .await
    .unwrap();

    // Will produce events to query.
    let negotiation = negotiate_agreement(
        &network,
        "Node-1",
        "Node-2",
        "negotiation",
        "r-session",
        "p-session",
    )
    .await
    .unwrap();

    let after_timestamp = negotiation.confirm_timestamp;

    let app = network.get_rest_app("Node-1").await;
    let url = format!(
        "/market-api/v1/agreementEvents?{}",
        QueryParamsBuilder::default()
            .put("afterTimestamp", Some(after_timestamp))
            .put("appSessionId", Some("r-session"))
            .put("maxEvents", Some(10))
            .build()
    );
    let req = actix_web::test::TestRequest::get().uri(&url).to_request();
    let resp = actix_web::test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let events: Vec<AgreementOperationEvent> = read_response_json(resp).await;

    expect_approve(events, "After agreementEvents").unwrap();
    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_terminate_agreement() -> anyhow::Result<()> {
    enable_logs(false);
    let dir = temp_dir!("test_terminate_agreement")?;
    let dir = dir.path();

    let network = MarketsNetwork::new(dir, MockNet::new())
        .await
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;

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

    let req_id = network.get_default_id(REQ_NAME).await;
    let prov_id = network.get_default_id(PROV_NAME).await;

    let reason = Some(Reason {
        message: "coś".into(),
        extra: serde_json::json!({"ala":"ma kota"}),
    });
    let url = format!(
        "/market-api/v1/agreements/{}/terminate",
        negotiation.r_agreement.into_client(),
    );
    log::info!("Requesting url: {}", url);
    let req = actix_web::test::TestRequest::post()
        .uri(&url)
        .set_json(&reason)
        .to_request();
    let app = network.get_rest_app(REQ_NAME).await;
    let resp = actix_web::test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);

    assert_eq!(
        network
            .get_market(REQ_NAME)
            .get_agreement(&negotiation.r_agreement, &req_id)
            .await
            .unwrap()
            .state,
        client_agreement::State::Terminated
    );
    assert_eq!(
        network
            .get_market(PROV_NAME)
            .get_agreement(&negotiation.p_agreement, &prov_id)
            .await
            .unwrap()
            .state,
        client_agreement::State::Terminated
    );
    Ok(())
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_terminate_agreement_without_reason() -> anyhow::Result<()> {
    enable_logs(false);
    let dir = temp_dir!("test_terminate_agreement_without_reason")?;
    let dir = dir.path();

    let network = MarketsNetwork::new(dir, MockNet::new())
        .await
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;

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

    let req_id = network.get_default_id(REQ_NAME).await;
    let prov_id = network.get_default_id(PROV_NAME).await;

    let reason = Option::<Reason>::None;
    let url = format!(
        "/market-api/v1/agreements/{}/terminate",
        negotiation.r_agreement.into_client(),
    );
    log::info!("Requesting url: {}", url);
    let req = actix_web::test::TestRequest::post()
        .uri(&url)
        .set_json(&reason)
        .to_request();
    let app = network.get_rest_app(REQ_NAME).await;
    let resp = actix_web::test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);

    assert_eq!(
        network
            .get_market(REQ_NAME)
            .get_agreement(&negotiation.r_agreement, &req_id)
            .await
            .unwrap()
            .state,
        client_agreement::State::Terminated
    );
    assert_eq!(
        network
            .get_market(PROV_NAME)
            .get_agreement(&negotiation.p_agreement, &prov_id)
            .await
            .unwrap()
            .state,
        client_agreement::State::Terminated
    );
    Ok(())
}

/// Agreement rejection happy path.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_rest_agreement_rejected() -> anyhow::Result<()> {
    enable_logs(false);
    let dir = temp_dir!("test_rest_agreement_rejected")?;
    let dir = dir.path();

    let network = MarketsNetwork::new(dir, MockNet::new())
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
    let req_id = network.get_default_id(REQ_NAME).await;
    let prov_id = network.get_default_id(PROV_NAME).await;

    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + chrono::Duration::milliseconds(300),
        )
        .await
        .unwrap();

    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await
        .unwrap();

    let url = format!(
        "/market-api/v1/agreements/{}/reject",
        agreement_id.into_client(),
    );
    let req = actix_web::test::TestRequest::post()
        .uri(&url)
        .set_json(Some(gen_reason("Not-interested")))
        .to_request();
    let app = network.get_rest_app(PROV_NAME).await;
    let resp = actix_web::test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);

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
    Ok(())
}

/// Agreement cancellation happy path.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_rest_agreement_cancelled() -> anyhow::Result<()> {
    enable_logs(false);
    let dir = temp_dir!("test_rest_agreement_cancelled")?;
    let dir = dir.path();

    let network = MarketsNetwork::new(dir, MockNet::new())
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
    let req_id = network.get_default_id(REQ_NAME).await;
    let prov_id = network.get_default_id(PROV_NAME).await;

    let agreement_id = req_engine
        .create_agreement(
            req_id.clone(),
            &proposal_id,
            Utc::now() + chrono::Duration::milliseconds(300),
        )
        .await
        .unwrap();

    req_engine
        .confirm_agreement(req_id.clone(), &agreement_id, None)
        .await
        .unwrap();

    let url = format!(
        "/market-api/v1/agreements/{}/cancel",
        agreement_id.into_client(),
    );
    let req = actix_web::test::TestRequest::post()
        .uri(&url)
        .set_json(Some(gen_reason("Changed my mind")))
        .to_request();
    let app = network.get_rest_app(REQ_NAME).await;
    let resp = actix_web::test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);

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
    Ok(())
}

// #[cfg_attr(not(feature = "test-suite"), ignore)]
// #[actix_rt::test]
// #[serial_test::serial]
// async fn test_rest_get_proposal_wrong_subscription() {
//     let dir = temp_dir!("test_rest_get_proposal_wrong_subscription")?;
//     let dir = dir.path();
//     let network = MarketsNetwork::new(dir, MockNet::new())
//         .await
//         .add_market_instance("Node-1")
//         .await.unwrap()
//         .add_market_instance("Node-2")
//         .await.unwrap();
//
//     let identity_local = network.get_default_id("Node-1");
//     let fake_subscription_id = SubscriptionId::generate_id(
//         "",
//         "",
//         &identity_local.identity,
//         &Utc::now().naive_utc(),
//         &Utc::now().naive_utc(),
//     );
//     let proposal_id = exchange_draft_proposals(&network, "Node-1", "Node-2").await.unwrap();
//     let mut app = network.get_rest_app("Node-1").await;
//
//     let req_offers = actix_web::test::TestRequest::get()
//         .uri(
//             format!(
//                 "/market-api/v1/offers/{}/proposals/{}",
//                 fake_subscription_id, proposal_id
//             )
//             .as_str(),
//         )
//         .to_request();
//     let resp_offers = actix_web::test::call_service(&mut app, req_offers).await;
//     assert_eq!(resp_offers.status(), StatusCode::NOT_FOUND);
//
//     let req_demands = actix_web::test::TestRequest::get()
//         .uri(
//             format!(
//                 "/market-api/v1/demands/{}/proposals/{}",
//                 fake_subscription_id, proposal_id
//             )
//             .as_str(),
//         )
//         .to_request();
//     let resp_demands = actix_web::test::call_service(&mut app, req_demands).await;
//
//     assert_eq!(resp_demands.status(), StatusCode::OK);
//     assert_eq!(resp_offers.status(), StatusCode::NOT_FOUND);
//
//
// }

pub async fn read_response_json<B: MessageBody + std::marker::Unpin, T: DeserializeOwned>(
    resp: ServiceResponse<B>,
) -> T {
    serde_json::from_slice(&actix_web::test::read_body(resp).await).unwrap()
}
