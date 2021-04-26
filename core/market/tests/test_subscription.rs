use ya_market::assert_err_eq;
use ya_market::testing::client::{sample_demand, sample_offer};
use ya_market::testing::mock_offer::flatten_json;
use ya_market::testing::{DemandError, QueryOfferError};
use ya_market::testing::{MarketServiceExt, MarketsNetwork};

/// Test subscribes offers, checks if offer is available
/// and than unsubscribes. Checking broadcasting behavior is out of scope.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_subscribe_offer() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await;

    let market1 = network.get_market("Node-1");
    let identity1 = network.get_default_id("Node-1");

    let offer = sample_offer();
    let subscription_id = market1.subscribe_offer(&offer, &identity1).await.unwrap();

    // Offer should be available in database after subscribe.
    let got_offer = market1.get_offer(&subscription_id).await.unwrap();
    let client_offer = got_offer.into_client_offer().unwrap();
    assert_eq!(client_offer.offer_id, subscription_id.to_string());
    assert_eq!(client_offer.provider_id, identity1.identity);
    assert_eq!(client_offer.constraints, offer.constraints);
    assert_eq!(client_offer.properties, flatten_json(&offer.properties));

    // Unsubscribe should fail on not existing subscription id.
    let not_existent_subscription_id = "00000000000000000000000000000001-0000000000000000000000000000000000000000000000000000000000000002".parse().unwrap();
    assert!(market1
        .unsubscribe_offer(&not_existent_subscription_id, &identity1)
        .await
        .is_err());

    market1
        .unsubscribe_offer(&subscription_id, &identity1)
        .await
        .unwrap();

    // Offer shouldn't be available after unsubscribed.
    assert_err_eq!(
        QueryOfferError::Unsubscribed(subscription_id.clone()),
        market1.get_offer(&subscription_id).await
    );
}

/// Test subscribes demand, checks if demand is available
/// and than unsubscribes. Checking broadcasting behavior is out of scope.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_subscribe_demand() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await;

    let market1 = network.get_market("Node-1");
    let identity1 = network.get_default_id("Node-1");

    let demand = sample_demand();
    let subscription_id = market1.subscribe_demand(&demand, &identity1).await.unwrap();

    // Offer should be available in database after subscribe.
    let got_demand = market1.get_demand(&subscription_id).await.unwrap();
    let client_demand = got_demand.into_client_demand().unwrap();
    assert_eq!(client_demand.demand_id, subscription_id.to_string());
    assert_eq!(client_demand.requestor_id, identity1.identity);
    assert_eq!(client_demand.constraints, demand.constraints);
    assert_eq!(client_demand.properties, flatten_json(&demand.properties));
    // Unsubscribe should fail on not existing subscription id.
    let not_existent_subscription_id = "00000000000000000000000000000002-0000000000000000000000000000000000000000000000000000000000000003".parse().unwrap();
    assert!(market1
        .unsubscribe_demand(&not_existent_subscription_id, &identity1)
        .await
        .is_err());

    market1
        .unsubscribe_demand(&subscription_id, &identity1)
        .await
        .unwrap();

    // Offer should be removed from database after unsubscribed.
    assert_err_eq!(
        DemandError::NotFound(subscription_id.clone()),
        market1.get_demand(&subscription_id).await
    );
}
