use bigdecimal::BigDecimal;
use test_context::test_context;
use ya_client_model::payment::allocation::PaymentPlatformEnum;
use ya_client_model::payment::NewAllocation;

use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::temp_dir;

use crate::mocks::node::MockNode;
use crate::mocks::payment::Driver;

mod mocks;

#[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn tutorial_how_to_use_module_tests(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(true);

    let dir = temp_dir!("tutorial_how_to_use_module_tests")?;

    // Create MockNode which is container for all Golem modules and represents
    // single node in tests.
    let node = MockNode::new("node-1", &dir.path())
        // Request wrappers around real Golem modules.
        .with_identity()
        .with_payment()
        // Mock market module with very basic implementation, which will allow to manually
        // create fake Agreements without need for Offers broadcasting and negotiation process.
        .with_fake_market();

    // Bind GSB and start server like yagna node would do in full setup.
    // Those functions will bind only modules chosen for MockNode.
    node.bind_gsb().await?;
    node.start_server(ctx).await?;

    let appkey = node.get_identity()?.create_identity_key("test").await?;

    node.get_payment()?
        .fund_account(Driver::Erc20, &appkey.identity.to_string())
        .await?;

    let api = node.rest_payments(&appkey.key)?;

    let _allocation = api
        .create_allocation(&NewAllocation {
            address: None, // Use default address (i.e. identity)
            payment_platform: Some(PaymentPlatformEnum::PaymentPlatformName(
                "erc20-holesky-tglm".to_string(),
            )),
            total_amount: BigDecimal::from(10u64),
            timeout: None,
            make_deposit: false,
            deposit: None,
            extend_timeout: None,
        })
        .await
        .unwrap();

    Ok(())
}
