use chrono::Utc;
use futures::{channel::mpsc, prelude::*};
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::time::Duration;

use ya_market::assert_err_eq;
use ya_market::testing::discovery::{message::*, Discovery};
use ya_market::testing::mock_offer::{client, sample_offer, sample_offer_with_expiration};
use ya_market::testing::{wait_for_bcast, MarketServiceExt, MarketsNetwork};
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

    // Add Offer on Node-1. It should be propagated to remaining nodes.
    let market1 = network.get_market("Node-1");
    let id1 = network.get_default_id("Node-1");

    let subscription_id = market1
        .subscribe_offer(&client::sample_offer(), &id1)
        .await
        .unwrap();
    let offer = market1.get_offer(&subscription_id).await.unwrap();

    // Expect, that Offer will appear on other nodes.
    let market2 = network.get_market("Node-2");
    let market3 = network.get_market("Node-3");
    wait_for_bcast(1000, &market2, &subscription_id, true).await;
    assert_eq!(offer, market2.get_offer(&subscription_id).await.unwrap());
    assert_eq!(offer, market3.get_offer(&subscription_id).await.unwrap());

    // Unsubscribe Offer. Wait some delay for propagation.
    market1
        .unsubscribe_offer(&subscription_id, &id1)
        .await
        .unwrap();
    let expected_error = QueryOfferError::Unsubscribed(subscription_id.clone());
    assert_err_eq!(expected_error, market1.get_offer(&subscription_id).await);
    // Expect, that Offer will disappear on other nodes.
    wait_for_bcast(1000, &market2, &subscription_id, false).await;
    assert_err_eq!(expected_error, market2.get_offer(&subscription_id).await);
    assert_err_eq!(expected_error, market2.get_offer(&subscription_id).await);
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

    let market1 = network.get_market("Node-1");

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

    wait_for_bcast(1000, &market1, &offer_id, true).await;

    let offer = market1.get_offer(&offer_id).await.unwrap();
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

    let market1 = network.get_market("Node-1");

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

    // Offer should be propagated to market1, but he should reject it.
    discovery2
        .bcast_offers(vec![invalid_id.clone()])
        .await
        .unwrap();

    tokio::time::delay_for(Duration::from_millis(1000)).await;
    assert_err_eq!(
        QueryOfferError::NotFound(invalid_id.clone()),
        market1.get_offer(&invalid_id).await,
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

    let market1 = network.get_market("Node-1");

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

    // Offer should be propagated to market1, but he should reject it.
    discovery2
        .bcast_offers(vec![offer_id.clone()])
        .await
        .unwrap();

    tokio::time::delay_for(Duration::from_millis(1000)).await;

    // This should return NotFound, because Market shouldn't add this Offer
    // to database at all.
    assert_err_eq!(
        QueryOfferError::NotFound(offer_id.clone()),
        market1.get_offer(&offer_id).await,
    );
}

/// Nodes shouldn't broadcast unsubscribed Offers.
/// This test broadcasts unsubscribed Offer and checks how other market Nodes
/// behave. We expect that market nodes will stop broadcast and Discovery interface will
/// get Offer only from himself.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_broadcast_stop_conditions() {
    let _ = env_logger::builder().try_init();
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await;

    // Add Offer on Node-1. It should be propagated to remaining nodes.
    let market1 = network.get_market("Node-1");
    let identity1 = network.get_default_id("Node-1");

    let subscription_id = market1
        .subscribe_offer(&client::sample_offer(), &identity1)
        .await
        .unwrap();
    let offer = market1.get_offer(&subscription_id).await.unwrap();

    // Expect, that Offer will appear on other nodes.
    let market2 = network.get_market("Node-2");
    wait_for_bcast(1000, &market2, &subscription_id, true).await;
    assert_eq!(offer, market2.get_offer(&subscription_id).await.unwrap());

    // Unsubscribe Offer. It should be unsubscribed on all Nodes and removed from
    // database on Node-2, since it's foreign Offer.
    market1
        .unsubscribe_offer(&subscription_id, &identity1)
        .await
        .unwrap();
    assert_err_eq!(
        QueryOfferError::Unsubscribed(subscription_id.clone()),
        market1.get_offer(&subscription_id).await
    );

    // Expect, that Offer will disappear on other nodes.
    wait_for_bcast(1000, &market2, &subscription_id, false).await;
    assert_err_eq!(
        QueryOfferError::Unsubscribed(subscription_id.clone()),
        market2.get_offer(&subscription_id).await
    );

    // Send the same Offer using Discovery interface directly.
    // Number of returning Offers will be counted.
    let (tx, mut rx) = mpsc::channel::<()>(1);
    let offers_counter = Arc::new(AtomicUsize::new(0));
    let counter = offers_counter.clone();

    let discovery_builder =
        network
            .discovery_builder()
            .add_handler(move |_caller: String, _msg: OffersBcast| {
                let offers_counter = counter.clone();
                let mut tx = tx.clone();
                async move {
                    offers_counter.fetch_add(1, Ordering::SeqCst);
                    tx.send(()).await.unwrap();
                    Ok(vec![])
                }
            });
    let network = network
        .add_discovery_instance("Node-3", discovery_builder)
        .await;

    // Broadcast already unsubscribed Offer. We will count number of Offers that will come back.
    let discovery3: Discovery = network.get_discovery("Node-3");
    discovery3
        .bcast_offers(vec![offer.id.clone()])
        .await
        .unwrap();

    // Wait for broadcast.
    tokio::time::timeout(Duration::from_millis(250), rx.next())
        .await
        .unwrap();

    assert_eq!(
        offers_counter.load(Ordering::SeqCst),
        1,
        "We expect to receive Offer only from ourselves"
    );

    // We expect, that Offers won't be available on other nodes now
    assert_err_eq!(
        QueryOfferError::Unsubscribed(subscription_id.clone()),
        market1.get_offer(&subscription_id).await,
    );
    assert_err_eq!(
        QueryOfferError::Unsubscribed(subscription_id.clone()),
        market2.get_offer(&subscription_id).await,
    );
}

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

    let market1 = network.get_market("Node-1");
    let id1 = network.get_default_id("Node-1");
    let discovery2 = network.get_discovery("Node-2");

    let subscription_id = market1
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
