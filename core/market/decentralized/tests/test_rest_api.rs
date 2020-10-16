use actix_web::{
    body::MessageBody, dev::ServiceResponse, error::PathError, http::StatusCode, test,
};
use chrono::Utc;
use serde::de::DeserializeOwned;
use serde_json::json;

use ya_client::model::market::Agreement;
use ya_client::model::{market::Demand, market::Offer, market::Proposal, ErrorMessage};
use ya_market_decentralized::testing::{
    client::{sample_demand, sample_offer},
    mock_node::{wait_for_bcast, MarketServiceExt},
    proposal_util::exchange_draft_proposals,
    DemandError, MarketsNetwork, ModifyOfferError, OwnerType, SubscriptionId,
    SubscriptionParseError,
};
use ya_market_resolver::flatten::flatten_json;

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_rest_get_offers() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await?
        .add_market_instance("Node-2")
        .await?;

    let market_local = network.get_market("Node-1");
    // Not really remote, but in this scenario will treat it as remote
    let market_remote = network.get_market("Node-2");
    let identity_local = network.get_default_id("Node-1");
    let identity_remote = network.get_default_id("Node-2");

    let offer_local = Offer::new(json!({}), "()".to_string());
    let offer_local_unsubscribed = Offer::new(json!({}), "()".to_string());
    let offer_remote = Offer::new(json!({}), "()".to_string());
    let subscription_id_local = market_local
        .subscribe_offer(&offer_local, &identity_local)
        .await?;
    let subscription_id_local_unsubscribed = market_local
        .subscribe_offer(&offer_local_unsubscribed, &identity_local)
        .await?;
    market_local
        .unsubscribe_offer(&subscription_id_local_unsubscribed, &identity_local)
        .await?;
    let subscription_id_remote = market_remote
        .subscribe_offer(&offer_remote, &identity_remote)
        .await?;
    let offer_local = market_local.get_offer(&subscription_id_local).await?;
    let _offer_remote = market_remote.get_offer(&subscription_id_remote).await?;

    wait_for_bcast(1000, &market_remote, &subscription_id_local, true).await;

    let mut app = network.get_rest_app("Node-1").await;

    let req = test::TestRequest::get()
        .uri("/market-api/v1/offers")
        .to_request();
    let resp = test::call_service(&mut app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let result: Vec<Offer> = read_response_json(resp).await;
    assert_eq!(vec![offer_local.into_client_offer()?], result);
    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_rest_get_demands() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await?;

    let market_local = network.get_market("Node-1");
    let identity_local = network.get_default_id("Node-1");
    let demand_local = Demand::new(json!({}), "()".to_string());
    let subscription_id = market_local
        .subscribe_demand(&demand_local, &identity_local)
        .await?;
    let demand_local = market_local.get_demand(&subscription_id).await?;

    let mut app = network.get_rest_app("Node-1").await;

    let req = test::TestRequest::get()
        .uri("/market-api/v1/demands")
        .to_request();
    let resp = test::call_service(&mut app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let result: Vec<Demand> = read_response_json(resp).await;
    assert_eq!(vec![demand_local.into_client_demand()?], result);

    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_rest_invalid_subscription_id_should_return_400() -> anyhow::Result<()> {
    // given
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await?;
    let mut app = network.get_rest_app("Node-1").await;

    let req = test::TestRequest::delete()
        .uri("/market-api/v1/offers/invalid-id")
        .to_request();

    // when
    let resp = test::call_service(&mut app, req).await;

    // then
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let result: ErrorMessage = read_response_json(resp).await;

    // let result = String::from_utf8(test::read_body(resp).await.to_vec()).unwrap();
    assert_eq!(
        PathError::Deserialize(serde::de::Error::custom(
            SubscriptionParseError::NotHexadecimal("invalid-id".to_string())
        ))
        .to_string(),
        result.message.unwrap()
    );
    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_rest_subscribe_unsubscribe_offer() -> anyhow::Result<()> {
    // given
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await?;
    let mut app = network.get_rest_app("Node-1").await;

    let mut client_offer = sample_offer();

    let req = test::TestRequest::post()
        .uri("/market-api/v1/offers")
        .set_json(&client_offer)
        .to_request();

    // when create offer
    let resp = test::call_service(&mut app, req).await;

    // then
    assert_eq!(resp.status(), StatusCode::CREATED);
    let subscription_id: SubscriptionId = read_response_json(resp).await;
    log::debug!("subscription_id: {}", subscription_id);

    // given
    let id = network.get_default_id("Node-1");
    client_offer.offer_id = Some(subscription_id.to_string());
    client_offer.provider_id = Some(id.identity.to_string());
    client_offer.properties = flatten_json(&client_offer.properties).unwrap();
    let market = network.get_market("Node-1");
    // when get from subscription store
    let offer = market.get_offer(&subscription_id).await.unwrap();
    // then
    assert_eq!(offer.into_client_offer(), Ok(client_offer));

    // given
    let req = test::TestRequest::delete()
        .uri(&format!("/market-api/v1/offers/{}", subscription_id))
        .to_request();
    // when unsubscribe
    let resp = test::call_service(&mut app, req).await;
    // then
    assert_eq!(resp.status(), StatusCode::OK);
    let result: String = read_response_json(resp).await;
    assert_eq!("Ok", result);

    // given
    let req = test::TestRequest::delete()
        .uri(&format!("/market-api/v1/offers/{}", subscription_id))
        .to_request();
    // when unsubscribe again
    let resp = test::call_service(&mut app, req).await;
    // then
    assert_eq!(resp.status(), StatusCode::GONE);
    let result: ErrorMessage = read_response_json(resp).await;
    // let result = String::from_utf8(test::read_body(resp).await.to_vec()).unwrap();
    assert_eq!(
        ModifyOfferError::AlreadyUnsubscribed(subscription_id.clone()).to_string(),
        result.message.unwrap()
    );
    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_rest_subscribe_unsubscribe_demand() -> anyhow::Result<()> {
    // given
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await?;
    let mut app = network.get_rest_app("Node-1").await;

    let mut client_demand = sample_demand();

    let req = test::TestRequest::post()
        .uri("/market-api/v1/demands")
        .set_json(&client_demand)
        .to_request();

    // when
    let resp = test::call_service(&mut app, req).await;

    // then
    assert_eq!(resp.status(), StatusCode::CREATED);
    let subscription_id: SubscriptionId = read_response_json(resp).await;
    log::debug!("subscription_id: {}", subscription_id);

    // given
    let id = network.get_default_id("Node-1");
    client_demand.demand_id = Some(subscription_id.to_string());
    client_demand.requestor_id = Some(id.identity.to_string());
    client_demand.properties = flatten_json(&client_demand.properties).unwrap();
    let market = network.get_market("Node-1");
    // when
    let demand = market.get_demand(&subscription_id).await.unwrap();
    // then
    assert_eq!(demand.into_client_demand(), Ok(client_demand));

    // given
    let req = test::TestRequest::delete()
        .uri(&format!("/market-api/v1/demands/{}", subscription_id))
        .to_request();
    // when
    let resp = test::call_service(&mut app, req).await;
    // then
    assert_eq!(resp.status(), StatusCode::OK);
    let result: String = read_response_json(resp).await;
    assert_eq!("Ok", result);

    // given
    let req = test::TestRequest::delete()
        .uri(&format!("/market-api/v1/demands/{}", subscription_id))
        .to_request();
    // when
    let resp = test::call_service(&mut app, req).await;
    // then
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let result: ErrorMessage = read_response_json(resp).await;
    // let result = String::from_utf8(test::read_body(resp).await.to_vec()).unwrap();
    assert_eq!(
        DemandError::NotFound(subscription_id.clone()).to_string(),
        result.message.unwrap()
    );
    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_rest_get_proposal() -> anyhow::Result<()> {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Provider")
        .await?
        .add_market_instance("Requestor")
        .await?;

    let prov_mkt = network.get_market("Provider");
    let proposal_id = exchange_draft_proposals(&network, "Requestor", "Provider")
        .await?
        .proposal_id
        .translate(OwnerType::Provider);

    // Not really remote, but in this scenario will treat it as remote
    let identity_local = network.get_default_id("Provider");
    let offers = prov_mkt.get_offers(Some(identity_local)).await?;
    let subscription_id = offers.first().unwrap().offer_id()?;
    let proposal = prov_mkt
        .get_proposal(&proposal_id)
        .await
        .unwrap()
        .into_client()?;
    let mut app = network.get_rest_app("Provider").await;

    let req_offers = test::TestRequest::get()
        .uri(
            format!(
                "/market-api/v1/offers/{}/proposals/{}",
                subscription_id, proposal_id
            )
            .as_str(),
        )
        .to_request();
    let resp_offers = test::call_service(&mut app, req_offers).await;
    assert_eq!(resp_offers.status(), StatusCode::OK);
    let result_offers: Proposal = read_response_json(resp_offers).await;
    assert_eq!(proposal, result_offers);

    let req_demands = test::TestRequest::get()
        .uri(
            format!(
                "/market-api/v1/demands/{}/proposals/{}",
                subscription_id, proposal_id
            )
            .as_str(),
        )
        .to_request();
    let resp_demands = test::call_service(&mut app, req_demands).await;
    assert_eq!(resp_demands.status(), StatusCode::OK);
    let resp_demands: Proposal = read_response_json(resp_demands).await;
    assert_eq!(proposal, resp_demands);
    Ok(())
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_rest_get_agreement() -> anyhow::Result<()> {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await?
        .add_market_instance("Node-2")
        .await?;

    let proposal_id = exchange_draft_proposals(&network, "Node-1", "Node-2")
        .await?
        .proposal_id;
    let req_market = network.get_market("Node-1");
    let req_engine = &req_market.requestor_engine;
    let req_id = network.get_default_id("Node-1");
    let prov_id = network.get_default_id("Node-2");

    let agreement_id = req_engine
        .create_agreement(req_id.clone(), &proposal_id, Utc::now())
        .await?;

    let mut app = network.get_rest_app("Node-1").await;
    let req = test::TestRequest::get()
        .uri(&format!(
            "/market-api/v1/agreements/{}",
            agreement_id.into_client()
        ))
        .to_request();
    let resp = test::call_service(&mut app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let agreement: Agreement = read_response_json(resp).await;
    assert_eq!(agreement.agreement_id, agreement_id.into_client());
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

// #[cfg_attr(not(feature = "market-test-suite"), ignore)]
// #[actix_rt::test]
// #[serial_test::serial]
// async fn test_rest_get_proposal_wrong_subscription() -> anyhow::Result<()> {
//     let network = MarketsNetwork::new(None)
//         .await
//         .add_market_instance("Node-1")
//         .await?
//         .add_market_instance("Node-2")
//         .await?;
//
//     let identity_local = network.get_default_id("Node-1");
//     let fake_subscription_id = SubscriptionId::generate_id(
//         "",
//         "",
//         &identity_local.identity,
//         &Utc::now().naive_utc(),
//         &Utc::now().naive_utc(),
//     );
//     let proposal_id = exchange_draft_proposals(&network, "Node-1", "Node-2").await?;
//     let mut app = network.get_rest_app("Node-1").await;
//
//     let req_offers = test::TestRequest::get()
//         .uri(
//             format!(
//                 "/market-api/v1/offers/{}/proposals/{}",
//                 fake_subscription_id, proposal_id
//             )
//             .as_str(),
//         )
//         .to_request();
//     let resp_offers = test::call_service(&mut app, req_offers).await;
//     assert_eq!(resp_offers.status(), StatusCode::NOT_FOUND);
//
//     let req_demands = test::TestRequest::get()
//         .uri(
//             format!(
//                 "/market-api/v1/demands/{}/proposals/{}",
//                 fake_subscription_id, proposal_id
//             )
//             .as_str(),
//         )
//         .to_request();
//     let resp_demands = test::call_service(&mut app, req_demands).await;
//
//     assert_eq!(resp_demands.status(), StatusCode::OK);
//     assert_eq!(resp_offers.status(), StatusCode::NOT_FOUND);
//
//     Ok(())
// }

pub async fn read_response_json<B: MessageBody, T: DeserializeOwned>(
    resp: ServiceResponse<B>,
) -> T {
    serde_json::from_slice(&test::read_body(resp).await).unwrap()
}
