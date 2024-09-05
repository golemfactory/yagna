use bigdecimal::BigDecimal;
use chrono::Utc;
use std::time::Duration;
use test_context::test_context;

use ya_client_model::payment::NewInvoice;
use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::{resource, temp_dir};
use ya_framework_mocks::market::FakeMarket;
use ya_framework_mocks::net::MockNet;
use ya_framework_mocks::node::MockNode;
use ya_framework_mocks::payment::fake_payment::FakePayment;
use ya_framework_mocks::payment::{Driver, PaymentRestExt};

#[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_payment_signature(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(true);

    let dir = temp_dir!("test_payment_signature")?;
    let dir = dir.path();

    let net = MockNet::new().bind();
    let node1 = MockNode::new(net.clone(), "node-1", dir)
        .with_identity()
        .with_payment(None)
        .with_fake_market();
    node1.bind_gsb().await?;
    node1.start_server(ctx).await?;

    let identity = node1.get_identity()?;
    let appkey_prov = identity.create_identity_key("provider").await?;
    let appkey_req = identity
        .create_from_private_key(&resource!("ci-requestor-1.key.priv"))
        .await?;

    node1
        .get_payment()?
        .fund_account(Driver::Erc20, &appkey_req.identity.to_string())
        .await?;

    let requestor = node1.rest_payments(&appkey_req.key)?;
    let provider = node1.rest_payments(&appkey_prov.key)?;

    log::info!("Creating mock Agreement...");
    let agreement =
        FakeMarket::create_fake_agreement(appkey_req.identity, appkey_prov.identity).unwrap();
    node1.get_market()?.add_agreement(agreement.clone()).await;

    log::info!("Creating allocation...");
    let new_allocation = FakePayment::default_allocation(&agreement, BigDecimal::from(10u64))?;
    let allocation = requestor.create_allocation(&new_allocation).await?;
    log::info!(
        "Allocation created. ({}) Issuing invoice...",
        allocation.allocation_id
    );

    let invoice = provider
        .issue_invoice(&NewInvoice {
            agreement_id: agreement.agreement_id.to_string(),
            activity_ids: None,
            amount: BigDecimal::from(2u64),
            payment_due_date: Utc::now(),
        })
        .await?;

    log::info!(
        "Invoice issued ({}). Sending invoice...",
        invoice.invoice_id
    );
    provider.send_invoice(&invoice.invoice_id).await?;

    log::info!(
        "Invoice sent. Accepting Invoice ({})...",
        invoice.invoice_id
    );
    requestor.get_invoice(&invoice.invoice_id).await.unwrap();
    requestor
        .simple_accept_invoice(&invoice, &allocation)
        .await
        .unwrap();

    // Payments are processed, and we don't want payment confirmation to reach Provider.
    // This is hack which will block communication between Requestor and Provider despite them
    // using the same node.
    // We want to send payment confirmation manually later. This way we will be able to modify
    // the message and check more different conditions.
    net.break_network_for(appkey_prov.identity);

    let payments = requestor
        .wait_for_invoice_payment::<Utc>(&invoice.invoice_id, Duration::from_secs(5 * 60), None)
        .await?;
    assert_eq!(payments.len(), 1);
    let _payment = requestor
        .get_signed_payment(&payments[0].payment_id)
        .await?;

    Ok(())
}
