use actix::prelude::*;
use serde_json::Value;
use std::path::Path;
use ya_agreement_utils::{
    ComInfo, InfNodeInfo, NodeInfo, OfferDefinition, OfferTemplate, ServiceInfo,
};
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
    let config = MarketConfig::from_env()?;
    let negotiators_cfg = create_agent_negotiators_config(data_dir)?;

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

//#[cfg_attr(not(feature = "system-test"), ignore)]
#[test_context::test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_provider_market(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(true);

    let dir = temp_dir!("test_provider_market")?;

    let node = start_mock_yagna(ctx, dir.path()).await?;
    let market = node.get_market()?;
    let appkey = node.get_identity()?.create_identity_key("provider").await?;

    let agent = create_provider_market(node.rest_market(&appkey.key)?, dir.path())?.start();

    agent
        .send(CreateOffer {
            offer_definition: OfferDefinition {
                node_info: NodeInfo::default(),
                srv_info: ServiceInfo::new(InfNodeInfo::default(), Value::Null)
                    .support_payload_manifest(false)
                    .support_multi_activity(true),
                com_info: ComInfo::default(),
                offer: OfferTemplate::default(),
            },
            preset: Preset::default(),
        })
        .await??;

    let offers = market.list_offer_subscriptions().await;
    assert_eq!(offers.len(), 1);

    // Expire Offer - this should trigger re-subscribing Offer by the Provider.
    let id = offers[0].offer.id.clone();
    market.expire_offer(&id).await?;

    log::info!("Offer expired and should be resubscribed by the Provider. Waiting...");
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    let offers = market.list_offer_subscriptions().await;
    assert_eq!(offers.len(), 2);

    agent.send(Shutdown {}).await??;
    Ok(())
}
