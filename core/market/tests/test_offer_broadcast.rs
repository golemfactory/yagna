use chrono::Utc;
use futures::{channel::mpsc, prelude::*};
use std::str::FromStr;
use tokio::time::Duration;

use ya_market::assert_err_eq;
use ya_market::testing::discovery::{message::*, Discovery};
use ya_market::testing::mock_node::{assert_offers_broadcasted, assert_unsunbscribes_broadcasted};
use ya_market::testing::mock_offer::{client, sample_offer, sample_offer_with_expiration};
use ya_market::testing::{MarketServiceExt, MarketsNetwork};
use ya_market::testing::{QueryOfferError, SubscriptionId};

/// Test adds offer. It should be broadcasted to other nodes in the network.
/// Than sending unsubscribe should remove Offer from other nodes.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_broadcast_offer() {
    let _ = env_logger::builder().try_init();
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await
        .add_market_instance("Node-3")
        .await;
    // make nodes are subscribed to broadcasts
    let mkt2 = network.get_market("Node-2");
    let id2 = network.get_default_id("Node-2");
    let mkt3 = network.get_market("Node-3");
    let id3 = network.get_default_id("Node-3");
    mkt2.subscribe_demand(&client::sample_demand(), &id2)
        .await
        .unwrap();
    mkt3.subscribe_demand(&client::sample_demand(), &id3)
        .await
        .unwrap();

    // Add Offer on Node-1. It should be propagated to remaining nodes.
    let mkt1 = network.get_market("Node-1");
    let id1 = network.get_default_id("Node-1");

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
}

/// This test checks, if Discovery interface calls expected sequence of callbacks.
/// In result Offer should be available on Node, that received broadcast.
/// Note: We don't need this test to check, if broadcasting works. test_broadcast_offer
/// is better, and higher level test for this purpose.
///
/// We check here, if valid Offer isn't rejected by market for some unknown reason.
/// If it is rejected, we can't trust other tests, that check if broadcasts validation
/// works correctly.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_broadcast_offer_callbacks() {
    let _ = env_logger::builder().try_init();
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await;

    let mkt1 = network.get_market("Node-1");
    let id1 = network.get_default_id("Node-1");
    // make sure mkt1 subscribes to broadcasts
    mkt1.subscribe_demand(&client::sample_demand(), &id1)
        .await
        .unwrap();

    let offer = sample_offer();
    let offer_clone = offer.clone();
    let offer_id = offer.id.clone();

    let discovery_builder = network.discovery_builder();
    let network = network
        .add_discovery_instance(
            "Node-2",
            discovery_builder.add_handler(move |_: String, _: RetrieveOffers| {
                let offer = offer.clone();
                async move { Ok(vec![offer]) }
            }),
        )
        .await;
    let discovery2: Discovery = network.get_discovery("Node-2");

    discovery2
        .bcast_offers(vec![offer_id.clone()])
        .await
        .unwrap();

    assert_offers_broadcasted(&[&mkt1], &[offer_id.clone()]).await;

    let offer = mkt1.get_offer(&offer_id).await.unwrap();
    assert_eq!(offer_clone, offer);
}

/// Offer subscription id should be validated on reception. If Offer
/// id hash doesn't match hash computed from Offer fields, Market should
/// reject such an Offer since it could be some kind of attack.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_broadcast_offer_id_validation() {
    let _ = env_logger::builder().try_init();
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await;

    let mkt1 = network.get_market("Node-1");

    // Prepare Offer with subscription id changed to invalid.
    let invalid_id = SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53").unwrap();
    let mut offer = sample_offer();
    offer.id = invalid_id.clone();

    let discovery_builder = network.discovery_builder();
    let network = network
        .add_discovery_instance(
            "Node-2",
            discovery_builder.add_handler(move |_: String, _: RetrieveOffers| {
                let offer = offer.clone();
                async move { Ok(vec![offer]) }
            }),
        )
        .await;
    let discovery2: Discovery = network.get_discovery("Node-2");

    // Offer should be propagated to mkt1, but he should reject it.
    discovery2
        .bcast_offers(vec![invalid_id.clone()])
        .await
        .unwrap();

    tokio::time::delay_for(Duration::from_millis(1000)).await;
    assert_err_eq!(
        QueryOfferError::NotFound(invalid_id.clone()),
        mkt1.get_offer(&invalid_id).await,
    );
}

/// Node should reject Offer, that already expired.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_broadcast_expired_offer() {
    let _ = env_logger::builder().try_init();
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await;

    let mkt1 = network.get_market("Node-1");

    // Prepare expired Offer to send.
    let expiration = Utc::now().naive_utc() - chrono::Duration::hours(1);
    let offer = sample_offer_with_expiration(expiration);
    let offer_id = offer.id.clone();

    let discovery_builder = network.discovery_builder();
    let network = network
        .add_discovery_instance(
            "Node-2",
            discovery_builder.add_handler(move |_: String, _: RetrieveOffers| {
                let offer = offer.clone();
                async move { Ok(vec![offer]) }
            }),
        )
        .await;
    let discovery2: Discovery = network.get_discovery("Node-2");

    // Offer should be propagated to mkt1, but he should reject it.
    discovery2
        .bcast_offers(vec![offer_id.clone()])
        .await
        .unwrap();

    tokio::time::delay_for(Duration::from_millis(1000)).await;

    // This should return NotFound, because Market shouldn't add this Offer
    // to database at all.
    assert_err_eq!(
        QueryOfferError::NotFound(offer_id.clone()),
        mkt1.get_offer(&offer_id).await,
    );
}

/// Note: Disabled after #1474 (Lazy broadcasts)
///// Nodes shouldn't broadcast unsubscribed Offers.
///// This test broadcasts unsubscribed Offer and checks how other market Nodes
///// behave. We expect that market nodes will stop broadcast and Discovery interface will
///// get Offer only from himself.
//#[cfg_attr(not(feature = "test-suite"), ignore)]
//#[serial_test::serial]
//async fn test_broadcast_stop_conditions() {
//    let _ = env_logger::builder().try_init();
//    let network = MarketsNetwork::new(None)
//        .await
//        .add_market_instance("Node-1")
//        .await
//        .add_market_instance("Node-2")
//        .await;
//
//    // Add Offer on Node-1. It should be propagated to remaining nodes.
//    let mkt1 = network.get_market("Node-1");
//    let identity1 = network.get_default_id("Node-1");
//
//    let mkt2 = network.get_market("Node-2");
//    let id2 = network.get_default_id("Node-2");
//    mkt1.subscribe_demand(&client::sample_demand(), &identity1).await.unwrap();
//    mkt2.subscribe_demand(&client::sample_demand(), &id2).await.unwrap();
//
//    let offer_id = mkt1
//        .subscribe_offer(&client::sample_offer(), &identity1)
//        .await
//        .unwrap();
//    let offer = mkt1.get_offer(&offer_id).await.unwrap();
//
//    // Expect, that Offer will appear on other nodes.
//    let mkt2 = network.get_market("Node-2");
//    assert_offers_broadcasted(&[&mkt2], &[offer_id.clone()]).await;
//    assert_eq!(offer, mkt2.get_offer(&offer_id).await.unwrap());
//
//    // Unsubscribe Offer. It should be unsubscribed on all Nodes and removed from
//    // database on Node-2, since it's foreign Offer.
//    mkt1.unsubscribe_offer(&offer_id, &identity1).await.unwrap();
//    assert_err_eq!(
//        QueryOfferError::Unsubscribed(offer_id.clone()),
//        mkt1.get_offer(&offer_id).await
//    );
//
//    // Expect, that Offer will disappear on other nodes.
//    assert_unsunbscribes_broadcasted(&[&mkt2], &[offer_id.clone()]).await;
//
//    // Send the same Offer using Discovery interface directly.
//    // Number of returning Offers will be counted.
//    let (tx, mut rx) = mpsc::channel::<()>(1);
//    let offers_counter = Arc::new(AtomicUsize::new(0));
//    let counter = offers_counter.clone();
//
//    let discovery_builder =
//        network
//            .discovery_builder()
//            .add_handler(move |_caller: String, _msg: OffersBcast| {
//                let offers_counter = counter.clone();
//                let mut tx = tx.clone();
//                async move {
//                    offers_counter.fetch_add(1, Ordering::SeqCst);
//                    tx.send(()).await.unwrap();
//                    Ok(vec![])
//                }
//            });
//    let network = network
//        .add_discovery_instance("Node-3", discovery_builder)
//        .await;
//
//    // Broadcast already unsubscribed Offer. We will count number of Offers that will come back.
//    let discovery3: Discovery = network.get_discovery("Node-3");
//    discovery3
//        .bcast_offers(vec![offer.id.clone()])
//        .await
//        .unwrap();
//
//    // Wait for broadcast.
//    tokio::time::timeout(Duration::from_millis(1500), rx.next())
//        .await
//        .unwrap();
//
//    assert_eq!(
//        offers_counter.load(Ordering::SeqCst),
//        1,
//        "We expect to receive Offer only from ourselves"
//    );
//
//    // We expect, that Offers won't be available on other nodes now
//    assert_err_eq!(
//        QueryOfferError::Unsubscribed(offer_id.clone()),
//        mkt1.get_offer(&offer_id).await,
//    );
//    assert_err_eq!(
//        QueryOfferError::Unsubscribed(offer_id.clone()),
//        mkt2.get_offer(&offer_id).await,
//    );
//}

/// Discovery `RetrieveOffers` GSB endpoint should return only existing Offers.
/// Test sends RetrieveOffers requesting existing and not existing subscription.
/// Market is expected to return only existing Offer without any error.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_discovery_get_offers() {
    let _ = env_logger::builder().try_init();
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await;

    let discovery_builder = network.discovery_builder();
    let network = network
        .add_discovery_instance("Node-2", discovery_builder)
        .await;

    let mkt1 = network.get_market("Node-1");
    let id1 = network.get_default_id("Node-1");
    let discovery2 = network.get_discovery("Node-2");

    let subscription_id = mkt1
        .subscribe_offer(&client::sample_offer(), &id1)
        .await
        .unwrap();
    let invalid_subscription = "00000000000000000000000000000001-0000000000000000000000000000000000000000000000000000000000000002".parse().unwrap();

    let offers = discovery2
        .get_remote_offers(
            id1.identity.to_string(),
            vec![subscription_id.clone(), invalid_subscription],
            5,
        )
        .await
        .unwrap();

    assert_eq!(offers.len(), 1);
    assert_eq!(offers[0].id, subscription_id);
}

/// Ensure that node is ready to handle broadcast message with more offers than
/// `max_bcasted_offers` or more unsubscribes than `max_bcasted_unsubscribes`. We will use sets
/// larger than 32766 as it's SQLITE_MAX_VARIABLE_NUMBER as of 3.32.0 (2020-05-22).
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_broadcast_50k() {
    let _ = env_logger::builder().try_init();
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await;
    let mkt1 = network.get_market("Node-1");
    let id1 = network.get_default_id("Node-1");
    // make sure mkt1 subscribes to broadcasts
    mkt1.subscribe_demand(&client::sample_demand(), &id1)
        .await
        .unwrap();

    let (tx, mut rx) = mpsc::channel::<Vec<SubscriptionId>>(1);

    let discovery_builder =
        network
            .discovery_builder()
            .add_handler(move |_: String, msg: RetrieveOffers| {
                let mut tx = tx.clone();
                async move {
                    tx.send(msg.offer_ids).await.unwrap();
                    Ok(vec![])
                }
            });
    let network = network
        .add_discovery_instance("Node-2", discovery_builder)
        .await;

    let discovery2 = network.get_discovery("Node-2");

    let mut offers_50k: Vec<SubscriptionId> = vec![];
    log::debug!("generating offers");
    for _n in 0..50000 {
        let o = sample_offer();
        offers_50k.push(o.id);
    }
    offers_50k.sort_by(|a, b| a.to_string().partial_cmp(&b.to_string()).unwrap());

    log::debug!("bcast offers: {}", offers_50k.len());
    discovery2.bcast_offers(offers_50k.clone()).await.unwrap();

    // Wait for broadcast.
    log::debug!("wait for bcast");
    let mut requested_offers = tokio::time::timeout(Duration::from_millis(1500), rx.next())
        .await
        .unwrap()
        .unwrap();
    requested_offers.sort_by(|a, b| a.to_string().partial_cmp(&b.to_string()).unwrap());
    log::debug!("bcast received {}", requested_offers.len());
    assert_eq!(
        requested_offers,
        offers_50k[offers_50k.len() - 100..].to_vec()
    );
}
