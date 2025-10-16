use ya_framework_basic::log::enable_logs;
use ya_framework_basic::temp_dir;
use ya_framework_mocks::assert_err_eq;
use ya_framework_mocks::market::legacy::mock_node::{
    assert_offers_broadcasted, assert_unsunbscribes_broadcasted, MarketsNetwork,
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
