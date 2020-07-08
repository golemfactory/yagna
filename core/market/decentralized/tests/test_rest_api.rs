use actix_http::{body::Body, Request};
use actix_service::Service as ActixService;
use actix_web::{error::PathError, http::StatusCode, test, App};

use crate::utils::mock_node::{wait_for_bcast, MarketStore};
use crate::utils::MarketsNetwork;
use actix_web::body::MessageBody;
use actix_web::dev::ServiceResponse;
use serde::de::DeserializeOwned;
use serde_json::json;
use ya_client::model::market::Offer;
use ya_client::model::{ErrorMessage, NodeId};
use ya_core_model::market;
use ya_market_decentralized::testing::{
    DemandError, OfferError, SubscriptionParseError, SubscriptionStore,
};
use ya_market_decentralized::{MarketService, SubscriptionId};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::{auth::dummy::DummyAuth, Identity};

mod utils;

//#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_rest_get_offers() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new("test_rest_get_offers")
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

    let mut app = test::init_service(
        App::new()
            .wrap(DummyAuth::new(identity_local))
            .service(MarketService::bind_rest(market_local)),
    )
    .await;

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
async fn test_rest_invalid_subscription_id_should_return_400() {
    // env_logger::init();

    // given
    let (_db, mut app) = init_db_app("test_rest_non_existent").await;

    // when
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
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_rest_subscribe_unsubscribe_offer() {
    // env_logger::init();

    // given
    let (db, mut app) = init_db_app("test_rest_subscribe_offer").await;

    let mut client_offer = utils::example_offer();

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
    client_offer.offer_id = Some(subscription_id.to_string());
    client_offer.provider_id = Some(mock_id().identity.to_string());
    // when get from subscription store
    let offer = SubscriptionStore::new(db)
        .get_offer(&subscription_id)
        .await
        .unwrap();
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
        OfferError::AlreadyUnsubscribed(subscription_id.clone()).to_string(),
        result.message.unwrap()
    );
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_rest_subscribe_unsubscribe_demand() {
    // env_logger::init();

    // given
    let (db, mut app) = init_db_app("test_rest_subscribe_demand").await;

    let mut client_demand = utils::example_demand();

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
    client_demand.demand_id = Some(subscription_id.to_string());
    client_demand.requestor_id = Some(mock_id().identity.to_string());
    // when
    let demand = SubscriptionStore::new(db)
        .get_demand(&subscription_id)
        .await
        .unwrap();
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
}

async fn init_db_app(
    test_name: &str,
) -> (
    DbExecutor,
    impl ActixService<
        Request = Request,
        Response = ServiceResponse<Body>,
        Error = actix_http::error::Error,
    >,
) {
    utils::mock_net::MockNet::new().unwrap();

    let test_dir = utils::mock_node::prepare_test_dir(test_name).unwrap();
    let db = DbExecutor::from_data_dir(&test_dir, "yagna").unwrap();

    let market = MarketService::new(&db).unwrap();

    market
        .bind_gsb(market::BUS_ID, market::private::BUS_ID)
        .await
        .unwrap();

    let app = test::init_service(
        App::new()
            .wrap(mock_auth())
            .service(MarketService::bind_rest(market.into())),
    )
    .await;

    (db, app)
}

fn mock_auth() -> DummyAuth {
    DummyAuth::new(mock_id())
}

fn mock_id() -> Identity {
    Identity {
        identity: "0xbabe000000000000000000000000000000000000"
            .parse::<NodeId>()
            .unwrap(),
        name: "".to_string(),
        role: "".to_string(),
    }
}

pub async fn read_response_json<B: MessageBody, T: DeserializeOwned>(
    resp: ServiceResponse<B>,
) -> T {
    serde_json::from_slice(&test::read_body(resp).await).unwrap()
}
