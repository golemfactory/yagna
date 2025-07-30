use actix::prelude::*;
use serde_json::Value;
use std::path::Path;
use ya_agreement_utils::{
    ComInfo, InfNodeInfo, NodeInfo, OfferDefinition, OfferTemplate, ServiceInfo,
};
use ya_framework_mocks::market::{MatcherError, SubscribeOfferResponse};
use ya_provider::market::{CreateOffer, Preset};

use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::temp_dir;
use ya_framework_mocks::net::MockNet;
use ya_framework_mocks::node::MockNode;

use ya_client::market::MarketProviderApi;
use ya_provider::market::config::MarketConfig;
use ya_provider::market::provider_market::{ProviderMarket, Shutdown};
use ya_provider::provider_agent::AgentNegotiatorsConfig;
use ya_provider::rules::RulesManager;

fn create_agent_negotiators_config(data_dir: &Path) -> anyhow::Result<AgentNegotiatorsConfig> {
    let rules_file = data_dir.join("rules.json");
    let whitelist_file = data_dir.join("domain_whitelist.json");
    let cert_dir = data_dir.join("cert-dir");
    std::fs::create_dir_all(&cert_dir)?;

    let rules_manager = RulesManager::load_or_create(&rules_file, &whitelist_file, &cert_dir)?;
    Ok(AgentNegotiatorsConfig { rules_manager })
}

fn create_provider_market(
    rest_api: MarketProviderApi,
    data_dir: &Path,
) -> anyhow::Result<ProviderMarket> {
    let mut config = MarketConfig::from_env()?;
    let negotiators_cfg = create_agent_negotiators_config(data_dir)?;
    config.negotiation_events_interval = 5.0;

    Ok(ProviderMarket::new(rest_api, config, negotiators_cfg))
}

async fn start_mock_yagna(ctx: &mut DroppableTestContext, dir: &Path) -> anyhow::Result<MockNode> {
    let net = MockNet::new().bind();
    let node = MockNode::new(net, "node-1", dir)
        .with_identity()
        .with_fake_market();
    node.bind_gsb().await?;
    node.start_server(ctx).await?;

    Ok(node)
}

fn create_default_offer(name: &str) -> CreateOffer {
    CreateOffer {
        offer_definition: OfferDefinition {
            node_info: NodeInfo::with_name(name.to_string()),
            srv_info: ServiceInfo::new(InfNodeInfo::default(), Value::Null)
                .support_payload_manifest(false)
                .support_multi_activity(true),
            com_info: ComInfo::default(),
            offer: OfferTemplate::default(),
        },
        preset: Preset::default(),
    }
}

#[cfg_attr(not(feature = "system-test"), ignore)]
#[test_context::test_context(DroppableTestContext)]
#[serial_test::serial]
/// Provider should resubscribe Offer after it expires.
async fn test_offer_resubscription_after_expiration(
    ctx: &mut DroppableTestContext,
) -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("test_offer_resubscription_after_expiration")?;

    let node = start_mock_yagna(ctx, dir.path()).await?;
    let market = node.get_market()?;
    let appkey = node.get_identity()?.create_identity_key("provider").await?;

    let agent = create_provider_market(node.rest_market(&appkey.key)?, dir.path())?.start();

    agent.send(create_default_offer("first-offer")).await??;

    let offers = market.list_offer_subscriptions().await;
    assert_eq!(offers.len(), 1);

    // Provider should resubscribe Offer after it expires.
    let id = offers[0].offer.id.clone();
    market.expire_offer(&id).await?;

    log::info!("Offer expired and should be resubscribed by the Provider. Waiting...");
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    let offers = market.list_offer_subscriptions().await;
    assert_eq!(offers.len(), 2);

    log::info!("Shutting down agent.");
    agent.send(Shutdown {}).await??;
    Ok(())
}

#[cfg_attr(not(feature = "system-test"), ignore)]
#[test_context::test_context(DroppableTestContext)]
#[serial_test::serial]
/// Error when creating offer on GolemBase, should result in retry attempt with exponential backoff.
async fn test_offer_resubscription_retry_on_creation_error(
    ctx: &mut DroppableTestContext,
) -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("test_offer_resubscription_retry_on_creation_error")?;

    let node = start_mock_yagna(ctx, dir.path()).await?;
    let market = node.get_market()?;
    let appkey = node.get_identity()?.create_identity_key("provider").await?;

    let agent = create_provider_market(node.rest_market(&appkey.key)?, dir.path())?.start();

    agent.send(create_default_offer("first-offer")).await??;

    let offers = market.list_offer_subscriptions().await;
    assert_eq!(offers.len(), 1);

    // Error when creating offer on GolemBase, should result in retry attempt with exponential backoff.
    log::info!("Running scenario 2: retrying when creating Offer on market failed");
    let id = offers[0].offer.id.clone();

    // Let's trigger a failure during next Offer creation. When the Offer expires later,
    // re-subscription will be triggered, but it will fail.
    // Provider Agent should attempt to publish Offer after delay.
    let failure_callback = market
        .next_subscribe_offer_response(SubscribeOfferResponse::Error(
            MatcherError::GolemBaseOfferError("Timeout creating offer".to_string()).into(),
        ))
        .await;
    let success_callback = market
        .next_subscribe_offer_response(SubscribeOfferResponse::Success)
        .await;
    // Get callbacks first before triggering expiration to avoid race conditions.
    market.expire_offer(&id).await?;

    log::info!("Waiting for Offer expiration to trigger re-subscribe attempt.");
    log::info!("Re-publishing Offer is expected to fail.");
    failure_callback
        .wait_for_trigger(std::time::Duration::from_secs(40))
        .await
        .unwrap();

    log::info!("Publishing Offer failed on first attempt. Waiting for next attempt after delay..");
    success_callback
        .wait_for_trigger(std::time::Duration::from_secs(10))
        .await
        .unwrap();
    log::info!("Offer is published.");

    let offers = market.list_offer_subscriptions().await;
    assert_eq!(offers.len(), 2);

    log::info!("Confirming that Offer was published successfully.");

    log::info!("Shutting down agent.");
    agent.send(Shutdown {}).await??;
    Ok(())
}

#[cfg_attr(not(feature = "system-test"), ignore)]
#[test_context::test_context(DroppableTestContext)]
#[serial_test::serial]
/// When Offer retry is triggered and at the same time the preset change happens, we shouldn't
/// allow for 2 Offers to be published.
async fn test_preset_change_during_retry(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("test_preset_change_during_retry")?;

    let node = start_mock_yagna(ctx, dir.path()).await?;
    let market = node.get_market()?;
    let appkey = node.get_identity()?.create_identity_key("provider").await?;

    let agent = create_provider_market(node.rest_market(&appkey.key)?, dir.path())?.start();

    agent.send(create_default_offer("first-offer")).await??;

    let offers = market.list_offer_subscriptions().await;
    assert_eq!(offers.len(), 1);

    // When Offer retry is triggered and at the same time the preset change happens, we shouldn't
    // allow for 2 Offers to be published.
    let id = offers[0].offer.id.clone();
    let failure_callback = market
        .next_subscribe_offer_response(SubscribeOfferResponse::Error(
            MatcherError::GolemBaseOfferError("Timeout creating offer".to_string()).into(),
        ))
        .await;

    market.expire_offer(&id).await?;
    failure_callback
        .wait_for_trigger(std::time::Duration::from_secs(40))
        .await
        .unwrap();

    // At this moment agent already failed to subscribe Offer. We should wait a little bit more
    // to update preset during retry delay period.
    log::info!("Updating preset");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // New preset will trigger subscribing a new Offer. We don't wait for any event, because
    // it will be done after message sending returns.
    agent.send(create_default_offer("second-offer")).await??;

    log::info!(
        "Preset updated and Offer published. Checking if retry won't trigger a new subscription."
    );

    // We register a callback which in correct implementation should never be triggered.
    // Retry attempt should be cancelled if preset was changed.
    let callback_result = market
        .next_subscribe_offer_response(SubscribeOfferResponse::Success)
        .await
        .wait_for_trigger(std::time::Duration::from_secs(40))
        .await;
    assert!(callback_result.is_err());
    assert_eq!(
        callback_result.unwrap_err().to_string(),
        "Timeout 40s waiting for endpoint"
    );

    log::info!("Shutting down agent.");
    agent.send(Shutdown {}).await??;
    Ok(())
}
