use actix_http::{body::Body, Request};
use actix_service::Service as ActixService;
use actix_web::{
    error::PathError,
    http::{header, StatusCode},
    test, App,
};

use actix_web::body::MessageBody;
use actix_web::dev::ServiceResponse;
use serde::de::DeserializeOwned;
use ya_client::model::{ErrorMessage, NodeId};
use ya_core_model::market;
use ya_market_decentralized::testing::{DemandDao, OfferDao, SubscriptionParseError};
use ya_market_decentralized::{MarketService, SubscriptionId};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::{auth::dummy::DummyAuth, Identity};

mod utils;

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

    let mut offer = utils::example_offer();

    let req = test::TestRequest::post()
        .uri("/market-api/v1/offers")
        .header(header::CONTENT_TYPE, "application/json")
        .set_json(&offer)
        .to_request();

    // when
    let resp = test::call_service(&mut app, req).await;

    // then
    assert_eq!(resp.status(), StatusCode::CREATED);
    let subscription_id: SubscriptionId = read_response_json(resp).await;
    log::debug!("subscription_id: {}", subscription_id);

    // given
    offer.offer_id = Some(subscription_id.to_string());
    offer.provider_id = Some(mock_id().identity.to_string());
    // when
    let model_offer = db
        .as_dao::<OfferDao>()
        .get_offer(&subscription_id)
        .await
        .unwrap()
        .unwrap();
    // then
    assert_eq!(model_offer.into_client_offer(), Ok(offer));

    // given
    let req = test::TestRequest::delete()
        .uri(&format!("/market-api/v1/offers/{}", subscription_id))
        .to_request();
    // when
    let resp = test::call_service(&mut app, req).await;
    // then
    assert_eq!(resp.status(), StatusCode::OK);
    let result: String = read_response_json(resp).await;
    assert_eq!("Ok", result);

    // given
    let req = test::TestRequest::delete()
        .uri(&format!("/market-api/v1/offers/{}", subscription_id))
        .to_request();
    // when
    let resp = test::call_service(&mut app, req).await;
    // then
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let result: ErrorMessage = read_response_json(resp).await;
    // let result = String::from_utf8(test::read_body(resp).await.to_vec()).unwrap();
    assert_eq!(
        format!(
            "Failed to unsubscribe Offer [{}]. Error: Offer already unsubscribed.",
            subscription_id
        ),
        result.message.unwrap()
    );
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_rest_subscribe_unsubscribe_demand() {
    // env_logger::init();

    // given
    let (db, mut app) = init_db_app("test_rest_subscribe_demand").await;

    let mut demand = utils::example_demand();

    let req = test::TestRequest::post()
        .uri("/market-api/v1/demands")
        .header(header::CONTENT_TYPE, "application/json")
        .set_json(&demand)
        .to_request();

    // when
    let resp = test::call_service(&mut app, req).await;

    // then
    assert_eq!(resp.status(), StatusCode::CREATED);
    let subscription_id: SubscriptionId = read_response_json(resp).await;
    log::debug!("subscription_id: {}", subscription_id);

    // given
    demand.demand_id = Some(subscription_id.to_string());
    demand.requestor_id = Some(mock_id().identity.to_string());
    // when
    let model_demand = db
        .as_dao::<DemandDao>()
        .get_demand(&subscription_id)
        .await
        .unwrap()
        .unwrap();
    // then
    assert_eq!(model_demand.into_client_demand(), Ok(demand));

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
        format!("Demand [{}] doesn\'t exist.", subscription_id),
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
    utils::mock_net::MockNet::gsb().unwrap();

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
