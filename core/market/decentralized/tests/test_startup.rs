use chrono::Duration;
use std::sync::Arc;

use ya_market_decentralized::testing::mock_offer::client;
use ya_market_decentralized::testing::Config;
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

    let network = network.add_market_instance("Node-3").await?;

    // We expect, that after this time we, should get at least one broadcast
    // from each Node.
    tokio::time::delay_for(std::time::Duration::from_millis(300)).await;

    let market3 = network.get_market("Node-3");

    // Make sure we got all offers that, were created.
    for subscription in subscriptions1.into_iter() {
        market3.get_offer(&subscription).await?;
    }
    for subscription in subscriptions2.into_iter() {
        market3.get_offer(&subscription).await?;
    }

    Ok(())
}
