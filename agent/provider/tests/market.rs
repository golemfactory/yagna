use std::path::Path;

use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::temp_dir;
use ya_framework_mocks::net::MockNet;
use ya_framework_mocks::node::MockNode;

use ya_client::market::MarketProviderApi;
use ya_provider::market::config::MarketConfig;
use ya_provider::market::provider_market::ProviderMarket;
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
    enable_logs(false);

    let dir = temp_dir!("test_provider_market")?;

    let node = start_mock_yagna(ctx, dir.path()).await?;
    let appkey = node.get_identity()?.create_identity_key("provider").await?;

    let agent = create_provider_market(node.rest_market(&appkey.key)?, dir.path())?;

    Ok(())
}
