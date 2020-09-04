use rand::seq::SliceRandom;
use std::sync::Arc;
use std::time::Duration;

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
    config.discovery.mean_cyclic_bcast_interval = Duration::from_millis(100);
    config.discovery.max_bcasted_offers = 50;

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
    tokio::time::delay_for(Duration::from_millis(400)).await;

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
    config.discovery.mean_cyclic_bcast_interval = Duration::from_millis(100);
    config.discovery.mean_cyclic_unsubscribes_interval = Duration::from_millis(100);
    config.discovery.max_bcasted_offers = 50;
    config.discovery.max_bcasted_unsubscribes = 50;

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
    tokio::time::delay_for(Duration::from_millis(200)).await;

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
    tokio::time::delay_for(Duration::from_millis(100)).await;
    network.enable_networking_for("Node-3")?;
    tokio::time::delay_for(Duration::from_millis(400)).await;

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

/// Subscribing and unsubscribing should work despite network errors.
/// Market should return subscription id and Offer Propagation will take place
/// later during cyclic broadcasts. The same applies to unsubscribes.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_network_error_while_subscribing() -> Result<(), anyhow::Error> {
    let _ = env_logger::builder().try_init();
    let network = MarketsNetwork::new("test_network_error_while_subscribing")
        .await
        .add_market_instance("Node-1")
        .await?
        .add_market_instance("Node-2")
        .await?;

    let market1 = network.get_market("Node-1");
    let id1 = network.get_default_id("Node-1");

    network.break_networking_for("Node-1")?;

    // It's not an error. Should pass.
    let subscription_id = market1
        .subscribe_offer(&client::sample_offer(), &id1)
        .await?;

    market1.unsubscribe_offer(&subscription_id, &id1).await?;
    Ok(())
}

/// Nodes send in cyclic broadcasts not only own Offers, but Offers
/// from other Nodes either.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_sharing_someones_else_offers() -> Result<(), anyhow::Error> {
    let _ = env_logger::builder().try_init();

    // Change expected time of sending broadcasts.
    let mut config = Config::default();
    config.discovery.mean_cyclic_bcast_interval = Duration::from_millis(100);
    config.discovery.max_bcasted_offers = 50;

    let network = MarketsNetwork::new("test_sharing_someones_else_offers")
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

    // Wait until Node-1 and Node-2 will share their Offers.
    tokio::time::delay_for(Duration::from_millis(200)).await;

    // Sanity check. Node-2 should have all Offers from market1.
    for subscription in subscriptions.iter() {
        assert!(market2.get_offer(subscription).await.is_ok());
    }

    // Break networking for Node-1. Only Node-2 will be able to send Offers.
    network.break_networking_for("Node-1")?;

    let network = network.add_market_instance("Node-3").await?;
    let market3 = network.get_market("Node-3");

    // We expect, that after this time we, should get at least one broadcast.
    tokio::time::delay_for(Duration::from_millis(400)).await;

    // Make sure we got all offers that, were created.
    for subscription in subscriptions.into_iter() {
        market3.get_offer(&subscription).await?;
    }
    Ok(())
}

/// Nodes send in cyclic broadcasts not only own Offers unsubscribes, but Offers
/// from other Nodes either.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_sharing_someones_else_unsubscribes() -> Result<(), anyhow::Error> {
    let _ = env_logger::builder().try_init();

    // Change expected time of sending broadcasts.
    let mut config = Config::default();
    config.discovery.mean_cyclic_bcast_interval = Duration::from_millis(100);
    config.discovery.mean_cyclic_unsubscribes_interval = Duration::from_millis(100);
    config.discovery.max_bcasted_offers = 50;
    config.discovery.max_bcasted_unsubscribes = 50;

    let network = MarketsNetwork::new("test_sharing_someones_else_unsubscribes")
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

    // Wait until Nodes will share their Offers.
    tokio::time::delay_for(Duration::from_millis(200)).await;

    // Sanity check. Node-3 should have all Offers from market1.
    for subscription in subscriptions.iter() {
        assert!(market2.get_offer(subscription).await.is_ok());
    }

    // Break networking for Node-3, so he won't see any unsubscribes.
    network.break_networking_for("Node-3")?;

    for subscription in subscriptions[30..].iter() {
        market2.unsubscribe_offer(subscription, &id2).await?;
    }

    tokio::time::delay_for(Duration::from_millis(50)).await;

    // Disconnect Node-2. Only Node-1 can propagate unsubscribes to Node-3.
    network.break_networking_for("Node-2")?;
    network.enable_networking_for("Node-3")?;

    // We expect that all unsubscribed will be shared with Node-3 after this delay.
    tokio::time::delay_for(Duration::from_millis(400)).await;
    for subscription in subscriptions[30..].into_iter() {
        let expected_error = QueryOfferError::Unsubscribed(subscription.clone());
        assert_err_eq!(expected_error, market3.get_offer(&subscription).await)
    }

    // All other Offers should remain untouched.
    for subscription in subscriptions[0..30].into_iter() {
        market3.get_offer(&subscription).await?;
    }
    Ok(())
}
