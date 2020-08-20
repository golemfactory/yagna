use ya_market_decentralized::testing::client::{sample_demand, sample_offer};
use ya_market_decentralized::testing::{DemandError, QueryOfferError};
use ya_market_decentralized::testing::{MarketServiceExt, MarketsNetwork};

macro_rules! assert_err_eq {
    ($expected:expr, $actual:expr $(,)*) => {
        assert_eq!($expected.to_string(), $actual.unwrap_err().to_string())
    };
}

/// Test subscribes offers, checks if offer is available
/// and than unsubscribes. Checking broadcasting behavior is out of scope.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_subscribe_offer() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new("test_subscribe_offer")
        .await
        .add_market_instance("Node-1")
        .await?;

    let market1 = network.get_market("Node-1");
    let identity1 = network.get_default_id("Node-1");

    let mut offer = sample_offer();
    let subscription_id = market1.subscribe_offer(&offer, &identity1).await?;

    // Fill expected values for further comparison.
    offer.provider_id = Some(identity1.identity.to_string());
    offer.offer_id = Some(subscription_id.to_string());

    // Offer should be available in database after subscribe.
    let got_offer = market1.get_offer(&subscription_id).await?;
    assert_eq!(got_offer.into_client_offer().unwrap(), offer);

    // Unsubscribe should fail on not existing subscription id.
    let not_existent_subscription_id = "00000000000000000000000000000001-0000000000000000000000000000000000000000000000000000000000000002".parse().unwrap();
    assert!(market1
        .unsubscribe_offer(&not_existent_subscription_id, &identity1)
        .await
        .is_err());

    market1
        .unsubscribe_offer(&subscription_id, &identity1)
        .await?;

    // Offer shouldn't be available after unsubscribed.
    assert_err_eq!(
        QueryOfferError::Unsubscribed(subscription_id.clone()),
        market1.get_offer(&subscription_id).await
    );

    Ok(())
}

/// Test subscribes demand, checks if demand is available
/// and than unsubscribes. Checking broadcasting behavior is out of scope.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_subscribe_demand() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new("test_subscribe_demand")
        .await
        .add_market_instance("Node-1")
        .await?;

    let market1 = network.get_market("Node-1");
    let identity1 = network.get_default_id("Node-1");

    let mut demand = sample_demand();
    let subscription_id = market1.subscribe_demand(&demand, &identity1).await?;

    // Fill expected values for further comparison.
    demand.requestor_id = Some(identity1.identity.to_string());
    demand.demand_id = Some(subscription_id.to_string());

    // Offer should be available in database after subscribe.
    let got_demand = market1.get_demand(&subscription_id).await?;
    assert_eq!(got_demand.into_client_demand().unwrap(), demand);

    // Unsubscribe should fail on not existing subscription id.
    let not_existent_subscription_id = "00000000000000000000000000000002-0000000000000000000000000000000000000000000000000000000000000003".parse().unwrap();
    assert!(market1
        .unsubscribe_demand(&not_existent_subscription_id, &identity1)
        .await
        .is_err());

    market1
        .unsubscribe_demand(&subscription_id, &identity1)
        .await?;

    // Offer should be removed from database after unsubscribed.
    assert_err_eq!(
        DemandError::NotFound(subscription_id.clone()),
        market1.get_demand(&subscription_id).await
    );

    Ok(())
}
