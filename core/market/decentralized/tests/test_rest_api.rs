use actix_http::{body::Body, Request};
use actix_service::Service as ActixService;
use actix_web::{error::PathError, http::StatusCode, test, App};

use actix_web::body::MessageBody;
use actix_web::dev::ServiceResponse;
use serde::de::DeserializeOwned;
use ya_client::model::{ErrorMessage, NodeId};
use ya_core_model::market;
use ya_market_decentralized::testing::{
    DemandError, OfferError, SubscriptionParseError, SubscriptionStore,
};
use ya_market_decentralized::{MarketService, SubscriptionId};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::auth::dummy::DummyAuth;

mod utils;

#[cfg(feature = "bcast-singleton")]
use utils::bcast::singleton::BCastService;
#[cfg(not(feature = "bcast-singleton"))]
use utils::bcast::BCastService;
use utils::{bcast::BCast, mock_net::MockNet};

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_rest_invalid_subscription_id_should_return_400() {
    // env_logger::init();

    // given
    let (_db, _node_id, mut app) = init_db_app("test_rest_non_existent").await;

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
    let (db, node_id, mut app) = init_db_app("test_rest_subscribe_offer").await;

    let mut client_offer = utils::sample_client_offer();

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
    client_offer.provider_id = Some(node_id.to_string());
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
    let (db, node_id, mut app) = init_db_app("test_rest_subscribe_demand").await;

    let mut client_demand = utils::sample_client_demand();

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
    client_demand.requestor_id = Some(node_id.to_string());
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
    NodeId,
    impl ActixService<
        Request = Request,
        Response = ServiceResponse<Body>,
        Error = actix_http::error::Error,
    >,
) {
    let id = utils::generate_identity(test_name);
    BCastService::default().register(&id.identity, test_name);
    MockNet::default().bind_gsb();

    let test_dir = utils::mock_node::prepare_test_dir(test_name).unwrap();
    let db = DbExecutor::from_data_dir(&test_dir, "yagna").unwrap();

    let market = MarketService::new(&db).unwrap();

    market
        .bind_gsb(market::BUS_ID, market::private::BUS_ID)
        .await
        .unwrap();

    let app = test::init_service(
        App::new()
            .wrap(DummyAuth::new(id.clone()))
            .service(MarketService::bind_rest(market.into())),
    )
    .await;

    (db, id.identity, app)
}

pub async fn read_response_json<B: MessageBody, T: DeserializeOwned>(
    resp: ServiceResponse<B>,
) -> T {
    serde_json::from_slice(&test::read_body(resp).await).unwrap()
}
