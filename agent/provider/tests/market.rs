use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::temp_dir;
use ya_framework_mocks::net::MockNet;
use ya_framework_mocks::node::MockNode;

use ya_provider::market::config::MarketConfig;
use ya_provider::market::provider_market::ProviderMarket;
use ya_provider::provider_agent::AgentNegotiatorsConfig;

//#[cfg_attr(not(feature = "system-test"), ignore)]
#[test_context::test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_provider_market(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("test_provider_market")?;

    let net = MockNet::new().bind();
    let node = MockNode::new(net, "node-1", dir.path())
        .with_identity()
        .with_fake_market();
    node.bind_gsb().await?;
    node.start_server(ctx).await?;

    let provider_appkey = node.get_identity()?.create_identity_key("provider").await?;

    let config = MarketConfig::default();
    let agent_negotiators_cfg = AgentNegotiatorsConfig::default();
    let provider_market = ProviderMarket::new(
        node.rest_market(&provider_appkey.key)?,
        config,
        agent_negotiators_cfg,
    );

    Ok(())
}
