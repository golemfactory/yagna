use chrono::Duration;
use rand::seq::SliceRandom;
use std::sync::Arc;

use ya_market_decentralized::assert_err_eq;
use ya_market_decentralized::testing::mock_offer::client;
use ya_market_decentralized::testing::Config;
use ya_market_decentralized::testing::QueryOfferError;
use ya_market_decentralized::testing::{MarketServiceExt, MarketsNetwork};

/// Initialize two markets and add Offers.
/// Third market that will be instantiated after these two, should
/// get all Offers from them, if cyclic broadcasting works properly.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_startup_offers_sharing() -> Result<(), anyhow::Error> {
    let _ = env_logger::builder().try_init();

    // Change expected time of sending broadcasts.
    let mut config = Config::default();
    config.discovery.mean_random_broadcast_interval = Duration::milliseconds(100);
    config.discovery.num_broadcasted_offers = 50;

    let network = MarketsNetwork::new("test_startup_offers_sharing")
        .await
        .with_config(Arc::new(config))
        .add_market_instance("Node-1")
        .await?
        .add_market_instance("Node-2")
        .await?;

    let market1 = network.get_market("Node-1");
    let id1 = network.get_default_id("Node-1");

    let market2 = network.get_market("Node-2");
    let id2 = network.get_default_id("Node-2");

    // Create some offers before we instantiate 3rd node.
    // This way this 3rd node won't get them in first broadcasts, that
    // are sent immediately, after subscription is made.
    let mut subscriptions = vec![];

    for _n in (0..30).into_iter() {
        subscriptions.push(
            market1
                .subscribe_offer(&client::sample_offer(), &id1)
                .await?,
        )
    }

    for _n in (0..20).into_iter() {
        subscriptions.push(
            market2
                .subscribe_offer(&client::sample_offer(), &id2)
                .await?,
        )
    }

    let network = network.add_market_instance("Node-3").await?;

    // We expect, that after this time we, should get at least one broadcast
    // from each Node.
    tokio::time::delay_for(std::time::Duration::from_millis(300)).await;

    let market3 = network.get_market("Node-3");

    // Make sure we got all offers that, were created.
    for subscription in subscriptions.into_iter() {
        market3.get_offer(&subscription).await?;
    }
    Ok(())
}

/// Unsubscribes are sent immediately after Offer is unsubscribed and
/// there are sent later in cyclic broadcasts. This test checks if cyclic broadcasts
/// are working correctly.
/// First initiate two Nodes with Offers, that will be shared with all 3 test Nodes.
/// Than break networking for one Node and in meantime unsubscribe some of Offers.
/// After networking will be reenabled, we expect, that 3rd Node will get all unsubscribes.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_unsubscribes_cyclic_broadcasts() -> Result<(), anyhow::Error> {
    let _ = env_logger::builder().try_init();

    // Change expected time of sending broadcasts.
    let mut config = Config::default();
    config.discovery.mean_random_broadcast_interval = Duration::milliseconds(100);
    config.discovery.mean_random_broadcast_unsubscribes_interval = Duration::milliseconds(100);
    config.discovery.num_broadcasted_offers = 50;
    config.discovery.num_broadcasted_unsubscribes = 50;

    let network = MarketsNetwork::new("test_unsubscribes_cyclic_broadcasts")
        .await
        .with_config(Arc::new(config))
        .add_market_instance("Node-1")
        .await?
        .add_market_instance("Node-2")
        .await?
        .add_market_instance("Node-3")
        .await?;

    let market1 = network.get_market("Node-1");
    let id1 = network.get_default_id("Node-1");

    let market2 = network.get_market("Node-2");
    let id2 = network.get_default_id("Node-2");

    let market3 = network.get_market("Node-3");

    // Create some offers before we instantiate 3rd node.
    // This way this 3rd node won't get them in first broadcasts, that
    // are sent immediately, after subscription is made.
    let mut subscriptions1 = vec![];
    let mut subscriptions2 = vec![];

    for _n in (0..30).into_iter() {
        subscriptions1.push(
            market1
                .subscribe_offer(&client::sample_offer(), &id1)
                .await?,
        )
    }

    for _n in (0..20).into_iter() {
        subscriptions2.push(
            market2
                .subscribe_offer(&client::sample_offer(), &id2)
                .await?,
        )
    }

    // We expect, that after this time all nodes will have the same knowledge about Offers.
    tokio::time::delay_for(std::time::Duration::from_millis(30)).await;

    // Break networking, so unsubscribe broadcasts won't come to Node-3.
    network.break_networking_for("Node-3")?;

    // Unsubscribe random Offers. First 10 elements of vectors will be unsubscribed.
    subscriptions1.shuffle(&mut rand::thread_rng());
    subscriptions2.shuffle(&mut rand::thread_rng());

    for subscription1 in subscriptions1[10..].iter() {
        market1.unsubscribe_offer(subscription1, &id1).await?;
    }
    for subscription2 in subscriptions2[10..].iter() {
        market2.unsubscribe_offer(subscription2, &id2).await?;
    }

    // Sanity check. We should have all Offers subscribe at this moment,
    // since our network didn't work.
    for subscription in subscriptions1.iter() {
        market3.get_offer(subscription).await?;
    }

    // Reenable networking for Node-3. We should get only cyclic broadcast.
    // Immediate broadcast should be already lost.
    tokio::time::delay_for(std::time::Duration::from_millis(100)).await;
    network.enable_networking_for("Node-3")?;
    tokio::time::delay_for(std::time::Duration::from_millis(300)).await;

    // Check if all expected Offers were unsubscribed.
    for subscription in subscriptions1[10..].into_iter() {
        let expected_error = QueryOfferError::Unsubscribed(subscription.clone());
        assert_err_eq!(expected_error, market3.get_offer(&subscription).await)
    }
    for subscription in subscriptions2[10..].into_iter() {
        let expected_error = QueryOfferError::Unsubscribed(subscription.clone());
        assert_err_eq!(expected_error, market3.get_offer(&subscription).await)
    }

    // Check Offers, that shouldn't be unsubscribed.
    for subscription in subscriptions1[0..10].into_iter() {
        market3.get_offer(&subscription).await?;
    }
    for subscription in subscriptions2[0..10].into_iter() {
        market3.get_offer(&subscription).await?;
    }

    Ok(())
}
