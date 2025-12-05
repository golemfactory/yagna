use std::sync::Arc;
use std::time::Duration;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::temp_dir;
use ya_framework_mocks::assert_err_eq;
use ya_framework_mocks::market::legacy::mock_node::{
    assert_offers_broadcasted, assert_unsunbscribes_broadcasted, create_market_config_for_test,
    MarketsNetwork,
};
use ya_framework_mocks::net::MockNet;

use ya_market::testing::{mock_offer::client, MarketServiceExt, QueryOfferError};

/// Test adds offer. It should be broadcasted to other nodes in the network.
/// Than sending unsubscribe should remove Offer from other nodes.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_broadcast_offer() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("test_broadcast_offer")?;
    let dir = dir.path();

    let network = MarketsNetwork::new(dir, MockNet::new())
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await
        .add_market_instance("Node-3")
        .await;
    // make nodes are subscribed to broadcasts
    let mkt2 = network.get_market("Node-2");
    let id2 = network.get_default_id("Node-2").await;
    let mkt3 = network.get_market("Node-3");
    let id3 = network.get_default_id("Node-3").await;
    mkt2.subscribe_demand(&client::sample_demand(), &id2)
        .await
        .unwrap();
    mkt3.subscribe_demand(&client::sample_demand(), &id3)
        .await
        .unwrap();

    // Add Offer on Node-1. It should be propagated to remaining nodes.
    let mkt1 = network.get_market("Node-1");
    let id1 = network.get_default_id("Node-1").await;

    let offer_id = mkt1
        .subscribe_offer(&client::sample_offer(), &id1)
        .await
        .unwrap();
    let offer = mkt1.get_offer(&offer_id).await.unwrap();

    // Expect, that Offer will appear on other nodes.
    let mkt2 = network.get_market("Node-2");
    let mkt3 = network.get_market("Node-3");
    assert_offers_broadcasted(&[&mkt2, &mkt3], &[offer_id.clone()]).await;
    assert_eq!(offer, mkt2.get_offer(&offer_id).await.unwrap());
    assert_eq!(offer, mkt3.get_offer(&offer_id).await.unwrap());

    // Unsubscribe Offer. Wait some delay for propagation.
    mkt1.unsubscribe_offer(&offer_id, &id1).await.unwrap();
    let expected_error = QueryOfferError::Unsubscribed(offer_id.clone());
    assert_err_eq!(expected_error, mkt1.get_offer(&offer_id).await);
    // Expect, that Offer will disappear on other nodes.
    assert_unsunbscribes_broadcasted(&[&mkt2, &mkt3], &[offer_id]).await;

    Ok(())
}

/// Test that if a node publishes an offer but the other node doesn't have a demand,
/// the offer shouldn't be available on the other node (lazy loading).
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_offer_not_available_without_demand() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("test_offer_not_available_without_demand")?;
    let dir = dir.path();

    let network = MarketsNetwork::new(dir, MockNet::new())
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await;

    // Node-1 publishes an offer
    let mkt1 = network.get_market("Node-1");
    let id1 = network.get_default_id("Node-1").await;
    let offer_id = mkt1
        .subscribe_offer(&client::sample_offer(), &id1)
        .await
        .unwrap();

    // Node-2 doesn't have a demand, so it shouldn't receive the offer
    let mkt2 = network.get_market("Node-2");

    // Wait a bit to ensure offers would have been propagated if listening was active
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Verify that Node-2 doesn't have the offer
    let offer_id_clone = offer_id.clone();
    let expected_error = QueryOfferError::NotFound(offer_id_clone);
    assert_err_eq!(expected_error, mkt2.get_offer(&offer_id).await);

    Ok(())
}

/// Test that if one node publishes a demand first, then after publishing an offer,
/// it should be available on the other node.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_offer_available_after_demand() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("test_offer_available_after_demand")?;
    let dir = dir.path();

    let network = MarketsNetwork::new(dir, MockNet::new())
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await;

    // Node-2 subscribes to a demand first (this should start listening)
    let mkt2 = network.get_market("Node-2");
    let id2 = network.get_default_id("Node-2").await;
    mkt2.subscribe_demand(&client::sample_demand(), &id2)
        .await
        .unwrap();

    // Give some time for the listener to start
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Node-1 publishes an offer
    let mkt1 = network.get_market("Node-1");
    let id1 = network.get_default_id("Node-1").await;
    let offer_id = mkt1
        .subscribe_offer(&client::sample_offer(), &id1)
        .await
        .unwrap();
    let offer = mkt1.get_offer(&offer_id).await.unwrap();

    // Node-2 should receive the offer since it's listening
    assert_offers_broadcasted(&[&mkt2], &[offer_id.clone()]).await;
    assert_eq!(offer, mkt2.get_offer(&offer_id).await.unwrap());

    Ok(())
}

/// Test that after removing a demand, the node won't receive any new offers,
/// but will receive all offers that were added during the period when it wasn't listening
/// once it starts listening again.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_stop_listening_after_demand_removal() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("test_stop_listening_after_demand_removal")?;
    let dir = dir.path();

    let network = MarketsNetwork::new(dir, MockNet::new())
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await;

    // Node-2 subscribes to a demand (starts listening)
    let mkt2 = network.get_market("Node-2");
    let id2 = network.get_default_id("Node-2").await;
    let demand_id = mkt2
        .subscribe_demand(&client::sample_demand(), &id2)
        .await
        .unwrap();

    // Give some time for the listener to start
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Node-1 publishes an offer while Node-2 is listening
    let mkt1 = network.get_market("Node-1");
    let id1 = network.get_default_id("Node-1").await;
    let offer_id_while_listening = mkt1
        .subscribe_offer(&client::sample_offer(), &id1)
        .await
        .unwrap();
    let offer_while_listening = mkt1.get_offer(&offer_id_while_listening).await.unwrap();

    // Node-2 should receive this offer
    assert_offers_broadcasted(&[&mkt2], &[offer_id_while_listening.clone()]).await;
    assert_eq!(
        offer_while_listening,
        mkt2.get_offer(&offer_id_while_listening).await.unwrap()
    );

    // Node-2 removes the demand (should stop listening)
    mkt2.unsubscribe_demand(&demand_id, &id2).await.unwrap();

    // Give some time for the listener to stop
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Node-1 publishes offers while Node-2 is NOT listening
    let offer_id_1_while_not_listening = mkt1
        .subscribe_offer(&client::sample_offer(), &id1)
        .await
        .unwrap();
    let offer_id_2_while_not_listening = mkt1
        .subscribe_offer(&client::sample_offer(), &id1)
        .await
        .unwrap();

    // Wait a bit to ensure offers would have been propagated if listening was active
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Node-2 should NOT receive these new offers since it's not listening anymore
    let expected_error_1 = QueryOfferError::NotFound(offer_id_1_while_not_listening.clone());
    assert_err_eq!(
        expected_error_1,
        mkt2.get_offer(&offer_id_1_while_not_listening).await
    );

    let expected_error_2 = QueryOfferError::NotFound(offer_id_2_while_not_listening.clone());
    assert_err_eq!(
        expected_error_2,
        mkt2.get_offer(&offer_id_2_while_not_listening).await
    );

    // Node-2 subscribes to a demand again (should start listening and receive missed offers)
    let _demand_id_2 = mkt2
        .subscribe_demand(&client::sample_demand(), &id2)
        .await
        .unwrap();

    // Give some time for the listener to start and query existing offers
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Node-2 should now receive all offers that were published while it wasn't listening
    assert_offers_broadcasted(
        &[&mkt2],
        &[
            offer_id_1_while_not_listening.clone(),
            offer_id_2_while_not_listening.clone(),
        ],
    )
    .await;

    // Verify Node-2 has all offers
    let offer_1 = mkt1
        .get_offer(&offer_id_1_while_not_listening)
        .await
        .unwrap();
    let offer_2 = mkt1
        .get_offer(&offer_id_2_while_not_listening)
        .await
        .unwrap();
    assert_eq!(
        offer_1,
        mkt2.get_offer(&offer_id_1_while_not_listening)
            .await
            .unwrap()
    );
    assert_eq!(
        offer_2,
        mkt2.get_offer(&offer_id_2_while_not_listening)
            .await
            .unwrap()
    );

    // Node-2 should still have the offer that was published while it was listening
    assert_eq!(
        offer_while_listening,
        mkt2.get_offer(&offer_id_while_listening).await.unwrap()
    );

    Ok(())
}

/// Test that after a demand expires, the node won't receive any new offers,
/// achieving the same effect as removing the last demand.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_stop_listening_after_demand_expiration() -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("test_stop_listening_after_demand_expiration")?;
    let dir = dir.path();

    // Create config with short demand TTL (3 seconds) - long enough for offers to be subscribed
    // but short enough to test expiration
    let mut config = create_market_config_for_test();
    config.subscription.default_ttl = chrono::Duration::seconds(3);

    let network = MarketsNetwork::new(dir, MockNet::new())
        .await
        .with_config(Arc::new(config))
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await;

    // Node-2 subscribes to a demand with short TTL (starts listening)
    let mkt2 = network.get_market("Node-2");
    let id2 = network.get_default_id("Node-2").await;
    mkt2.subscribe_demand(&client::sample_demand(), &id2)
        .await
        .unwrap();

    // Give some time for the listener to start
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Node-1 publishes an offer while Node-2 is listening
    let mkt1 = network.get_market("Node-1");
    let id1 = network.get_default_id("Node-1").await;
    let offer_id_while_listening = mkt1
        .subscribe_offer(&client::sample_offer(), &id1)
        .await
        .unwrap();
    let offer_while_listening = mkt1.get_offer(&offer_id_while_listening).await.unwrap();

    // Node-2 should receive this offer
    assert_offers_broadcasted(&[&mkt2], &[offer_id_while_listening.clone()]).await;
    assert_eq!(
        offer_while_listening,
        mkt2.get_offer(&offer_id_while_listening).await.unwrap()
    );

    // Wait for the demand to expire (TTL is 3 seconds, wait 3.5 seconds to be sure)
    tokio::time::sleep(Duration::from_millis(3500)).await;

    // Node-1 publishes another offer after demand expiration
    let offer_id_after_expiration = mkt1
        .subscribe_offer(&client::sample_offer(), &id1)
        .await
        .unwrap();

    // Wait a bit to ensure offers would have been propagated if listening was active
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Node-2 should NOT receive this new offer since demand expired and listening stopped
    let expected_error = QueryOfferError::NotFound(offer_id_after_expiration.clone());
    assert_err_eq!(
        expected_error,
        mkt2.get_offer(&offer_id_after_expiration).await
    );

    // The offer published while listening will also expire (same TTL as demand), but it shoudl be still in database.
    let expected_expired_error = QueryOfferError::Expired(offer_id_while_listening.clone());
    assert_err_eq!(
        expected_expired_error,
        mkt2.get_offer(&offer_id_while_listening).await
    );

    Ok(())
}

// /// Note: Test disabled since hybrid NET requires limiting number of Subscriptions.
// /// Unreliable broadcasts have limited packet size, because we don't want to implement
// /// packets fragmentation.
// /// Ensure that node is ready to handle broadcast message with more offers than
// /// `max_bcasted_offers` or more unsubscribes than `max_bcasted_unsubscribes`. We will use sets
// /// larger than 32766 as it's SQLITE_MAX_VARIABLE_NUMBER as of 3.32.0 (2020-05-22).
// #[cfg_attr(not(feature = "test-suite"), ignore)]
// #[serial_test::serial]
// async fn test_broadcast_50k() {
//     let _ = env_logger::builder().try_init();
//     let dir = temp_dir!("test_broadcast_50k")?;
//     let dir = dir.path();
//     let network = MarketsNetwork::new(dir, MockNet::new())
//         .await
//         .add_market_instance("Node-1")
//         .await;
//     let mkt1 = network.get_market("Node-1");
//     let id1 = network.get_default_id("Node-1");
//     // make sure mkt1 subscribes to broadcasts
//     mkt1.subscribe_demand(&client::sample_demand(), &id1)
//         .await
//         .unwrap();
//
//     let (tx, mut rx) = mpsc::channel::<Vec<SubscriptionId>>(1);
//
//     let discovery_builder =
//         network
//             .discovery_builder()
//             .add_handler(move |_: String, msg: RetrieveOffers| {
//                 let mut tx = tx.clone();
//                 async move {
//                     tx.send(msg.offer_ids).await.unwrap();
//                     Ok(vec![])
//                 }
//             });
//     let network = network
//         .add_discovery_instance("Node-2", discovery_builder)
//         .await;
//
//     let discovery2 = network.get_discovery("Node-2");
//
//     let mut offers_50k: Vec<SubscriptionId> = vec![];
//     log::debug!("generating offers");
//     for _n in 0..50000 {
//         let o = sample_offer();
//         offers_50k.push(o.id);
//     }
//     offers_50k.sort_by(|a, b| a.to_string().partial_cmp(&b.to_string()).unwrap());
//
//     log::debug!("bcast offers: {}", offers_50k.len());
//     discovery2.bcast_offers(offers_50k.clone()).await.unwrap();
//
//     // Wait for broadcast.
//     log::debug!("wait for bcast");
//     let mut requested_offers = tokio::time::timeout(Duration::from_millis(50000), rx.next())
//         .await
//         .unwrap()
//         .unwrap();
//     requested_offers.sort_by(|a, b| a.to_string().partial_cmp(&b.to_string()).unwrap());
//     log::debug!("bcast received {}", requested_offers.len());
//     assert_eq!(
//         requested_offers,
//         offers_50k[offers_50k.len() - 100..].to_vec()
//     );
// }
