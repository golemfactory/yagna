use rand::seq::SliceRandom;
use std::time::Duration;

use ya_market::assert_err_eq;
use ya_market::testing::{
    mock_node::{assert_offers_broadcasted, assert_unsunbscribes_broadcasted},
    mock_offer::client,
    MarketServiceExt, MarketsNetwork, QueryOfferError,
};

/// Disabled for #1474 (Lazy broadcasts)
/// Initialize two markets and add Offers.
/// Third market that will be instantiated after these two, should
/// get all Offers from them, if cyclic broadcasting works properly.
//#[cfg_attr(not(feature = "test-suite"), ignore)]
//#[serial_test::serial]
//async fn test_startup_offers_sharing() {
//    let _ = env_logger::builder().try_init();
//
//    let network = MarketsNetwork::new(None)
//        .await
//        .add_market_instance("Node-1")
//        .await
//        .add_market_instance("Node-2")
//        .await;
//
//    let mkt1 = network.get_market("Node-1");
//    let id1 = network.get_default_id("Node-1");
//
//    let mkt2 = network.get_market("Node-2");
//    let id2 = network.get_default_id("Node-2");
//
//    // Create some offers before we instantiate 3rd node.
//    // This way this 3rd node won't get them in first broadcasts, that
//    // are sent immediately, after subscription is made.
//    let mut subscriptions = vec![];
//
//    for _n in 0..3 {
//        subscriptions.push(
//            mkt1.subscribe_offer(&client::sample_offer(), &id1)
//                .await
//                .unwrap(),
//        )
//    }
//
//    for _n in 0..2 {
//        subscriptions.push(
//            mkt2.subscribe_offer(&client::sample_offer(), &id2)
//                .await
//                .unwrap(),
//        )
//    }
//
//    let network = network.add_market_instance("Node-3").await;
//
//    let mkt3 = network.get_market("Node-3");
//
//    // Make sure we got all offers that, were created.
//    assert_offers_broadcasted(&[&mkt1, &mkt2, &mkt3], &subscriptions).await;
//}

/// Unsubscribes are sent immediately after Offer is unsubscribed and
/// there are sent later in cyclic broadcasts. This test checks if cyclic broadcasts
/// are working correctly.
/// First initiate two Nodes with Offers, that will be shared with all 3 test Nodes.
/// Than break networking for one Node and in meantime unsubscribe some of Offers.
/// After networking will be reenabled, we expect, that 3rd Node will get all unsubscribes.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_unsubscribes_cyclic_broadcasts() {
    let _ = env_logger::builder().try_init();

    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await
        .add_market_instance("Node-3")
        .await;

    let mkt1 = network.get_market("Node-1");
    let id1 = network.get_default_id("Node-1");

    let mkt2 = network.get_market("Node-2");
    let id2 = network.get_default_id("Node-2");

    let mkt3 = network.get_market("Node-3");
    let id3 = network.get_default_id("Node-3");

    // create demands so that after #1474 nodes will be subscribed to broadcasts
    mkt1.subscribe_demand(&client::sample_demand(), &id1)
        .await
        .unwrap();
    mkt2.subscribe_demand(&client::sample_demand(), &id2)
        .await
        .unwrap();
    mkt3.subscribe_demand(&client::sample_demand(), &id3)
        .await
        .unwrap();

    let mut subscriptions1 = vec![];
    let mut subscriptions2 = vec![];

    for _n in 0..30 {
        subscriptions1.push(
            mkt1.subscribe_offer(&client::sample_offer(), &id1)
                .await
                .unwrap(),
        )
    }

    for _n in 0..20 {
        subscriptions2.push(
            mkt2.subscribe_offer(&client::sample_offer(), &id2)
                .await
                .unwrap(),
        )
    }

    assert_offers_broadcasted(
        &[&mkt1, &mkt2, &mkt3],
        subscriptions1.iter().chain(subscriptions2.iter()),
    )
    .await;

    // Break networking, so unsubscribe broadcasts won't come to Node-3.
    network.break_networking_for("Node-3").unwrap();

    // Unsubscribe random Offers.
    // Only the first elements of the vectors will NOT be unsubscribed.
    subscriptions1.shuffle(&mut rand::thread_rng());
    subscriptions2.shuffle(&mut rand::thread_rng());

    for subscription in subscriptions1[10..].iter() {
        mkt1.unsubscribe_offer(subscription, &id1).await.unwrap();
    }
    for subscription in subscriptions2[10..].iter() {
        mkt2.unsubscribe_offer(subscription, &id2).await.unwrap();
    }

    // Sanity check. We should have all Offers subscribed at this moment,
    // since Node-3 network didn't work.
    assert_offers_broadcasted(&[&mkt3], subscriptions1.iter().chain(subscriptions2.iter())).await;

    // Re-enable networking for Node-3. We should get only cyclic broadcast.
    // Immediate broadcast should be already lost.
    network.enable_networking_for("Node-3").unwrap();
    tokio::time::sleep(Duration::from_millis(320)).await;

    // Check if all expected Offers were unsubscribed.
    assert_unsunbscribes_broadcasted(
        &[&mkt1, &mkt2, &mkt3],
        subscriptions1[10..]
            .iter()
            .chain(subscriptions2[10..].iter()),
    )
    .await;

    // Check Offers, that shouldn't be unsubscribed.
    assert_offers_broadcasted(
        &[&mkt1, &mkt2, &mkt3],
        subscriptions1[0..10]
            .iter()
            .chain(subscriptions2[0..10].iter()),
    )
    .await;
}

/// Subscribing and unsubscribing should work despite network errors.
/// Market should return subscription id and Offer propagation will take place
/// later during cyclic broadcasts. The same applies to unsubscribes.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_network_error_while_subscribing() {
    let _ = env_logger::builder().try_init();
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await;

    let mkt1 = network.get_market("Node-1");
    let id1 = network.get_default_id("Node-1");

    network.break_networking_for("Node-1").unwrap();

    // It's not an error. Should pass.
    let subscription_id = mkt1
        .subscribe_offer(&client::sample_offer(), &id1)
        .await
        .unwrap();

    mkt1.unsubscribe_offer(&subscription_id, &id1)
        .await
        .unwrap();

    let expected_error = QueryOfferError::Unsubscribed(subscription_id.clone());
    assert_err_eq!(expected_error, mkt1.get_offer(&subscription_id).await);

    let expected_error = QueryOfferError::NotFound(subscription_id.clone());
    let mkt2 = network.get_market("Node-2");
    assert_err_eq!(expected_error, mkt2.get_offer(&subscription_id).await);
}

/// Note: Functionality disabled. Nodes send only own Offers.
/// Nodes send in cyclic broadcasts not only own Offers, but Offers
/// from other Nodes either.
// #[cfg_attr(not(feature = "test-suite"), ignore)]
// #[serial_test::serial]
// async fn test_sharing_someones_else_offers() {
//     let _ = env_logger::builder().try_init();
//
//     let network = MarketsNetwork::new(None)
//         .await
//         .add_market_instance("Node-1")
//         .await
//         .add_market_instance("Node-2")
//         .await;
//
//     let mkt1 = network.get_market("Node-1");
//     let id1 = network.get_default_id("Node-1");
//
//     let mkt2 = network.get_market("Node-2");
//     let id2 = network.get_default_id("Node-2");
//
//     // Create some offers before we instantiate 3rd node.
//     // This way this 3rd node won't get them in first broadcasts, that
//     // are sent immediately, after subscription is made.
//     let mut subscriptions = vec![];
//
//     for _n in 0..3 {
//         subscriptions.push(
//             mkt1.subscribe_offer(&client::sample_offer(), &id1)
//                 .await
//                 .unwrap(),
//         )
//     }
//
//     for _n in 0..2 {
//         subscriptions.push(
//             mkt2.subscribe_offer(&client::sample_offer(), &id2)
//                 .await
//                 .unwrap(),
//         )
//     }
//
//     // Sanity check. Both nodes should have all Offers.
//     assert_offers_broadcasted(&[&mkt1, &mkt2], subscriptions.iter()).await;
//
//     // Break networking for Node-1. Only Node-2 will be able to send Offers.
//     network.break_networking_for("Node-1").unwrap();
//     let network = network.add_market_instance("Node-3").await;
//     let mkt3 = network.get_market("Node-3");
//
//     // Make sure Node-3 has all offers from both: Node-1 and Node-2.
//     assert_offers_broadcasted(&[&mkt1, &mkt2, &mkt3], subscriptions.iter()).await;
// }

/// Nodes send in cyclic broadcasts not only own Offers unsubscribes, but unsubscribes
/// from other Nodes either.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_sharing_someones_else_unsubscribes() {
    let _ = env_logger::builder().try_init();

    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await
        .add_market_instance("Node-3")
        .await;

    let mkt1 = network.get_market("Node-1");
    let id1 = network.get_default_id("Node-1");

    let mkt2 = network.get_market("Node-2");
    let id2 = network.get_default_id("Node-2");

    let mkt3 = network.get_market("Node-3");
    let id3 = network.get_default_id("Node-3");

    let mut subscriptions = vec![];

    // create demands so that after #1474 nodes will be subscribed to broadcasts
    mkt1.subscribe_demand(&client::sample_demand(), &id1)
        .await
        .unwrap();
    mkt2.subscribe_demand(&client::sample_demand(), &id2)
        .await
        .unwrap();
    mkt3.subscribe_demand(&client::sample_demand(), &id3)
        .await
        .unwrap();
    for _n in 0..3 {
        subscriptions.push(
            mkt1.subscribe_offer(&client::sample_offer(), &id1)
                .await
                .unwrap(),
        )
    }

    for _n in 0..2 {
        subscriptions.push(
            mkt2.subscribe_offer(&client::sample_offer(), &id2)
                .await
                .unwrap(),
        )
    }

    // Wait until Nodes will share their Offers.
    // After 300ms we should get at least two broadcasts from each Node.
    tokio::time::sleep(Duration::from_millis(320)).await;

    // Sanity check. Make sure all nodes have all offers.
    assert_offers_broadcasted(&[&mkt1, &mkt2, &mkt3], subscriptions.iter()).await;

    // Break networking for Node-3, so he won't see any unsubscribes.
    network.break_networking_for("Node-3").unwrap();

    for subscription in subscriptions[3..].iter() {
        mkt2.unsubscribe_offer(subscription, &id2).await.unwrap();
    }

    // Check if all expected Offers were unsubscribed.
    assert_unsunbscribes_broadcasted(&[&mkt1, &mkt2], subscriptions[3..].iter()).await;
    // Sanity check. Node-3 should still see all Offers (not unsubscribed).
    assert_offers_broadcasted(&[&mkt3], subscriptions.iter()).await;

    // Disconnect Node-2. Only Node-1 can propagate unsubscribes to Node-3.
    network.break_networking_for("Node-2").unwrap();
    network.enable_networking_for("Node-3").unwrap();

    // We expect that all unsubscribed will be shared with Node-3 after this delay.
    assert_unsunbscribes_broadcasted(&[&mkt1, &mkt2, &mkt3], &subscriptions[3..]).await;

    // All other Offers should remain untouched.
    assert_offers_broadcasted(&[&mkt1, &mkt2, &mkt3], &subscriptions[0..3]).await;
}
