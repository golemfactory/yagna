use test_context::test_context;

use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::{resource, temp_dir};
use ya_framework_mocks::market::FakeMarket;
use ya_framework_mocks::net::MockNet;
use ya_framework_mocks::node::MockNode;
use ya_framework_mocks::payment::Driver;

#[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_payment_sync(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(true);

    let dir = temp_dir!("test_payment_sync")?;
    let dir = dir.path();

    let net = MockNet::new().bind();

    let node1 = MockNode::new(net.clone(), "node-1", dir)
        .with_identity()
        .with_payment()
        .with_fake_market();
    node1.bind_gsb().await?;
    node1.start_server(ctx).await?;

    let appkey_req = node1
        .get_identity()?
        .create_from_private_key(&resource!("ci-requestor-1.key.priv"))
        .await?;

    node1
        .get_payment()?
        .fund_account(Driver::Erc20, &appkey_req.identity.to_string())
        .await?;

    let node2 = MockNode::new(net, "node-2", dir)
        .with_prefixed_gsb()
        .with_identity()
        .with_fake_payment();
    node2.bind_gsb().await?;

    let appkey_prov = node2
        .get_identity()?
        .create_identity_key("provider")
        .await?;

    let agreement =
        FakeMarket::create_fake_agreement(appkey_req.identity, appkey_prov.identity).unwrap();
    node1.get_market()?.add_agreement(agreement.clone()).await;

    let _requestor = node1.rest_payments(&appkey_req.key)?;
    Ok(())
}
