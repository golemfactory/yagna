use bigdecimal::BigDecimal;
use std::str::FromStr;
use test_context::test_context;

use ya_client_model::payment::allocation::{PaymentPlatform, PaymentPlatformEnum};
use ya_client_model::payment::{Acceptance, NewAllocation};
use ya_core_model::payment::local::GetStatus;
use ya_core_model::payment::public::{AcceptInvoice, Ack, SendInvoice};
use ya_service_bus::RpcEndpoint;

use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::{resource, temp_dir};

use ya_framework_mocks::market::FakeMarket;
use ya_framework_mocks::net::MockNet;
use ya_framework_mocks::node::MockNode;
use ya_framework_mocks::payment::fake_payment::FakePayment;
use ya_framework_mocks::payment::Driver;

// Tests should be always wrapped in these macros.
// `serial_test` forces sequential execution. It is nor possible to run them concurrently, because
// we bind to single GSB.
// `DroppableTestContext` is a helper struct which will clean up after tests (for example shutdown servers etc).
#[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn tutorial_how_to_use_module_tests(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(true);

    // This line create temporary directory for test data, that will be removed after `TempDir` is dropped.
    // Directory will be placed in cargo `target/tmp`.
    // You can use `TempDir` library in 2 ways:
    // - `dir.path()` to get path which can be used further in tests
    // - `dir.into_path()` which steals PathBuf preventing temporary directory from removing on the end of the test.
    // Use the second option during debugging your tests and switch back to the first one before merging PR.
    let dir = temp_dir!("tutorial_how_to_use_module_tests")?;
    let dir = dir.path();

    // MockNet routes traffic between MockNodes.
    // Currently instantiating many MockNodes is not possible, but MockNet is necessary even
    // for communication on the same node, because messages directed to external GSB addresses `/net/0x437544...`
    // when NodeId belongs to local Node, need to be routed back.
    let net = MockNet::new().bind();

    // Create MockNode which is container for all Golem modules and represents
    // single node in tests.
    let node = MockNode::new(net.clone(), "node-1", dir)
        // Request instantiating wrappers around real Golem modules.
        .with_identity()
        .with_payment()
        // Mock market module with very basic implementation, which will allow to manually
        // create fake Agreements without need for Offers broadcasting and negotiation process.
        .with_fake_market();

    // Bind GSB and start server like yagna node would do in full setup.
    // Those functions will bind only modules chosen for MockNode.
    node.bind_gsb().await?;
    node.start_server(ctx).await?;

    // Creating identities is essential to use REST API and create Agreements and Payments.
    // Provider and Requestor should use separate identity.
    let identity = node.get_identity()?;
    // Requestor identity is created from pre-existing private key. Provider will use newly created identity.
    // Using the same identity exposes our private key, but these are testnet money anyway.
    // By doing this we can speed up tests significantly, because we don't have to wait for
    // wallet founding, which is rather long operation.
    let appkey_req = identity
        .create_from_private_key(&resource!("ci-requestor-1.key.priv"))
        .await?;
    let appkey_prov = identity.create_identity_key("provider").await?;

    // Fund Requestor account. In most case we already have funds on this wallet,
    // so this will be no-op.
    node.get_payment()?
        .fund_account(Driver::Erc20, &appkey_req.identity.to_string())
        .await?;

    // Create REST API client for give node, to test payments endpoints.
    let api = node.rest_payments(&appkey_req.key)?;

    let payment_platform = PaymentPlatform {
        driver: Some(Driver::Erc20.gsb_name()),
        network: Some("holesky".to_string()),
        token: Some("tglm".to_string()),
    };

    let _allocation = api
        .create_allocation(&NewAllocation {
            address: None, // Use default address (i.e. identity)
            payment_platform: Some(PaymentPlatformEnum::PaymentPlatform(
                payment_platform.clone(),
            )),
            total_amount: BigDecimal::from(10u64),
            timeout: None,
            make_deposit: false,
            deposit: None,
            extend_timeout: None,
        })
        .await
        .unwrap();

    // Some interactions will require mock Agreements between Requestor and Provider.
    let agreement =
        FakeMarket::create_fake_agreement(appkey_req.identity, appkey_prov.identity).unwrap();
    node.get_market()?.add_agreement(agreement.clone()).await;

    // Always use `gsb_local_endpoint()` to determine address on which to call GSB.
    // GSB addresses can be different from regular yagna usage when we will be able to instantiate
    // multiple MockNodes in single tests.
    let status = node
        .get_payment()?
        .gsb_local_endpoint()
        .call(GetStatus {
            address: appkey_req.identity.to_string(),
            driver: payment_platform.driver.clone().unwrap(),
            network: payment_platform.network.clone(),
            token: None,
            after_timestamp: 0,
        })
        .await??;

    log::info!(
        "Requestor balance: {}, allocated: {}/{}",
        status.amount,
        status.reserved,
        status.amount
    );

    // Instead of using real yagna node, we can use FakePayment instead.
    // It allows us to capture GSB traffic to test if real payment module sends them correctly.
    let node3 = MockNode::new(net, "node-3", dir)
        // All GSB handler will be bound on addresses prefixed by node name.
        // After creating identity, it will be registered in MockNet and all GSB messages
        // will be routed to those prefixes.
        .with_prefixed_gsb()
        .with_identity()
        .with_fake_payment();
    node3.bind_gsb().await?;

    let identity = node3.get_identity()?.create_identity_key("node-3").await?;

    let agreement =
        FakeMarket::create_fake_agreement(appkey_req.identity, identity.identity).unwrap();
    node.get_market()?.add_agreement(agreement.clone()).await;

    // Sending Invoice from FakePayment node to real payment module.
    // This can be done by directly sending GSB message directly, simulating, what would happen
    // after calling REST API.
    // `gsb_public_endpoint` returns GSB address on which `node` payment module was bound.
    // Note that you should never build GSB addresses directly, because `MockNode` implementation
    // decides about them, and they can be different from real yagna.
    let invoice = FakePayment::fake_invoice(&agreement, BigDecimal::from_str("0.2")?)?;
    node.get_payment()?
        .gsb_public_endpoint()
        .send_as(invoice.issuer_id, SendInvoice(invoice.clone()))
        .await??;

    // FakePayment responds always with correct answer to any GSB message.
    // This behavior can be overridden by querying `FakePayment::message_channel` and setting
    // expected response.
    // Function overrides default message handler and returns channel which yields all messages
    // received.
    let mut channel = node3
        .get_fake_payment()?
        .message_channel::<AcceptInvoice>(Ok(Ack {}));

    // Sending accept Invoice in real Payment module.
    // Internally GSB message will be sent to node3 and will be captured by channel.
    let new_allocation = FakePayment::default_allocation(&agreement, BigDecimal::from(10u64))?;
    let allocation = api.create_allocation(&new_allocation).await?;
    api.accept_invoice(
        &invoice.invoice_id,
        &Acceptance {
            total_amount_accepted: invoice.amount.clone(),
            allocation_id: allocation.allocation_id.to_string(),
        },
    )
    .await
    .unwrap();

    // Waiting for any message sent to `node3`.
    // We expect that `AcceptInvoice` message will be received after previous REST api call was made.
    let accept = channel.recv().await.unwrap();
    assert_eq!(accept.invoice_id, invoice.invoice_id);
    Ok(())
}
