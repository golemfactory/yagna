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

fn create_default_offer() -> CreateOffer {
    CreateOffer {
        offer_definition: OfferDefinition {
            node_info: NodeInfo::default(),
            srv_info: ServiceInfo::new(InfNodeInfo::default(), Value::Null)
                .support_payload_manifest(false)
                .support_multi_activity(true),
            com_info: ComInfo::default(),
            offer: OfferTemplate::default(),
        },
        preset: Preset::default(),
    }
}

//#[cfg_attr(not(feature = "system-test"), ignore)]
#[test_context::test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_provider_market_resubscribing(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(true);

    let dir = temp_dir!("test_provider_market")?;

    let node = start_mock_yagna(ctx, dir.path()).await?;
    let market = node.get_market()?;
    let appkey = node.get_identity()?.create_identity_key("provider").await?;

    let agent = create_provider_market(node.rest_market(&appkey.key)?, dir.path())?.start();

    agent.send(create_default_offer()).await??;

    let offers = market.list_offer_subscriptions().await;
    assert_eq!(offers.len(), 1);

    // Scenario 1:
    // Provider should resubscribe Offer after it expires.
    let id = offers[0].offer.id.clone();
    market.expire_offer(&id).await?;

    log::info!("Offer expired and should be resubscribed by the Provider. Waiting...");
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    let offers = market.list_offer_subscriptions().await;
    assert_eq!(offers.len(), 2);

    // Scenario 2:
    // Error when creating offer on GolemBase, should result in retry attempt with exponential backoff.
    log::info!("Running scenario 2: retrying when creating Offer on market failed");
    let id = offers[1].offer.id.clone();

    // Get callbacks first before triggering expiration to avoid race conditions.
    let failure_callback = market
        .next_subscribe_offer_response(SubscribeOfferResponse::Error(
            MatcherError::GolemBaseOfferError("Timeout creating offer".to_string()).into(),
        ))
        .await;
    let success_callback = market
        .next_subscribe_offer_response(SubscribeOfferResponse::Success)
        .await;
    market.expire_offer(&id).await?;

    log::info!("Waiting for Offer expiration to trigger re-subscribe attempt.");
    log::info!("Re-publishing Offer is expected to fail.");
    failure_callback
        .wait_for_trigger(std::time::Duration::from_secs(40))
        .await
        .unwrap();

    log::info!("Publishing Offer failed on first attempt. Waiting for retry..");
    success_callback
        .wait_for_trigger(std::time::Duration::from_secs(10))
        .await
        .unwrap();
    log::info!("Offer should be published at this moment.");

    let offers = market.list_offer_subscriptions().await;
    assert_eq!(offers.len(), 3);

    log::info!("Confirming that Offer was published successfully.");

    // Scenario 3:
    // When Offer retry is triggered and at the same time the preset change happens, we shouldn't
    // allow for 2 Offers to be published.
    let id = offers[2].offer.id.clone();
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

    log::info!("Updating preset");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    agent.send(create_default_offer()).await??;

    let success_callback = market
        .next_subscribe_offer_response(SubscribeOfferResponse::Success)
        .await;

    log::info!("Shutting down agent.");
    agent.send(Shutdown {}).await??;
    Ok(())
}
