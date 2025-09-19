use std::sync::Arc;
use std::time::Duration;

use ya_framework_basic::log::enable_logs;
use ya_framework_basic::temp_dir;
use ya_framework_mocks::assert_err_eq;
use ya_market::testing::mock_offer::client;

use ya_framework_mocks::market::legacy::mock_node::{
    create_market_config_for_test, MarketsNetwork,
};
use ya_framework_mocks::net::MockNet;
use ya_market::testing::{QueryEventsError, TakeEventsError};

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_golembase_offer_expiration_correct() -> Result<(), anyhow::Error> {
    enable_logs(false);

    let dir = temp_dir!("test_golembase_offer_expiration")?;
    let dir = dir.path();

    let mut config = create_market_config_for_test();
    config.subscription.default_ttl = chrono::Duration::seconds(5);
    let network = MarketsNetwork::new_containerized(dir, MockNet::new())
        .await
        .with_config(Arc::new(config))
        .add_market_instance("Node-1")
        .await;

    let mkt1 = network.get_market("Node-1");
    let id1 = network.get_default_id("Node-1").await;

    let id = mkt1
        .subscribe_offer(&client::sample_offer(), &id1)
        .await
        .unwrap();

    log::info!("Querying events before expiration shoudl succeed");
    mkt1.provider_engine
        .query_events(&id, 2.0, Some(5))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_secs(3)).await;

    log::info!("Querying events after expiration should fail");
    let result = mkt1.provider_engine.query_events(&id, 2.0, Some(5)).await;
    assert_err_eq!(
        QueryEventsError::TakeEvents(TakeEventsError::Expired(id)),
        result
    );
    Ok(())
}

// This scenarion reproduces problem, that despite Offer being expired,
// querying events won't return appropriate error. When this happens Provider will
// continue listening to events instead of resubscribing Offer. As a result Offer is not
// visible to the network, but Provider doesn't notice that.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_golembase_offer_expiration_desync_before() -> Result<(), anyhow::Error> {
    enable_logs(false);

    let dir = temp_dir!("test_golembase_offer_expiration")?;
    let dir = dir.path();

    // Set expiration to un-even number. Market will convert this number to mulitplicity
    // of 2s, which is block interval on Golem Base.
    // This will cause desynchronization between market expiration counter and Golem Base.
    let mut config = create_market_config_for_test();
    config.subscription.default_ttl = chrono::Duration::milliseconds(5300);
    let network = MarketsNetwork::new_mocked(dir, MockNet::new())
        .await?
        .with_config(Arc::new(config))
        .add_market_instance("Node-1")
        .await;

    let mkt1 = network.get_market("Node-1");
    let id1 = network.get_default_id("Node-1").await;

    let now = std::time::Instant::now();
    let id = mkt1
        .subscribe_offer(&client::sample_offer(), &id1)
        .await
        .unwrap();

    log::info!(
        "Querying events 1 after: {}",
        humantime::format_duration(now.elapsed())
    );
    let _events = mkt1
        .provider_engine
        .query_events(&id, 8.0, Some(5))
        .await
        .unwrap();

    log::info!(
        "Querying events 2 after: {}. Offer should have already expired.",
        humantime::format_duration(now.elapsed())
    );

    let result = mkt1.provider_engine.query_events(&id, 5.0, Some(5)).await;
    log::info!(
        "Query result finished after: {}. Expected failure due to expiration.",
        humantime::format_duration(now.elapsed())
    );
    assert_err_eq!(
        QueryEventsError::TakeEvents(TakeEventsError::Expired(id)),
        result
    );

    Ok(())
}

// The same scenario as `test_golembase_offer_expiration_desync_before` but this
// time expiration on Golem Base elapses later than market internal.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_golembase_offer_expiration_desync_after() -> Result<(), anyhow::Error> {
    enable_logs(false);

    let dir = temp_dir!("test_golembase_offer_expiration")?;
    let dir = dir.path();

    // Set expiration to un-even number. Market will convert this number to mulitplicity
    // of 2s, which is block interval on Golem Base.
    // This will cause desynchronization between market expiration counter and Golem Base.
    let mut config = create_market_config_for_test();
    config.subscription.default_ttl = chrono::Duration::milliseconds(6300);
    let network = MarketsNetwork::new_mocked(dir, MockNet::new())
        .await?
        .with_config(Arc::new(config))
        .add_market_instance("Node-1")
        .await;

    let mkt1 = network.get_market("Node-1");
    let id1 = network.get_default_id("Node-1").await;

    let now = std::time::Instant::now();
    let id = mkt1
        .subscribe_offer(&client::sample_offer(), &id1)
        .await
        .unwrap();

    log::info!(
        "Querying events 1 after: {}",
        humantime::format_duration(now.elapsed())
    );
    let _events = mkt1
        .provider_engine
        .query_events(&id, 10.0, Some(5))
        .await
        .unwrap();

    log::info!(
        "Querying events 2 after: {}. Offer should have already expired.",
        humantime::format_duration(now.elapsed())
    );

    let result = mkt1.provider_engine.query_events(&id, 5.0, Some(5)).await;
    log::info!(
        "Query result finished after: {}. Expected failure due to expiration.",
        humantime::format_duration(now.elapsed())
    );
    assert_err_eq!(
        QueryEventsError::TakeEvents(TakeEventsError::Expired(id)),
        result
    );

    Ok(())
}
