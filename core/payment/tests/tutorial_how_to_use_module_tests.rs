use test_context::test_context;

use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::temp_dir;

use mocks::node::MockNode;

mod mocks;

#[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn tutorial_how_to_use_module_tests(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(true);

    let dir = temp_dir!("tutorial_how_to_use_module_tests")?;

    let node = MockNode::new("node-1", &dir.path())
        .with_identity()
        .with_payment();
    node.bind_gsb().await?;
    node.start_server(ctx).await?;

    Ok(())
}
