use bigdecimal::BigDecimal;
use chrono::Utc;
use std::str::FromStr;
use std::time::Duration;
use test_context::test_context;
use tokio::sync::mpsc::error::TryRecvError;

use ya_core_model::payment::public::{
    AcceptInvoice, Ack, PaymentSync, PaymentSyncWithBytes, SendInvoice, SendPayment,
};
use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::{resource, temp_dir};
use ya_framework_mocks::market::FakeMarket;
use ya_framework_mocks::net::MockNet;
use ya_framework_mocks::node::MockNode;
use ya_framework_mocks::payment::fake_payment::FakePayment;
use ya_framework_mocks::payment::{Driver, PaymentRestExt};
use ya_service_bus::timeout::IntoTimeoutFuture;
use ya_service_bus::RpcEndpoint;

#[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_payment_sync(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("test_payment_sync")?;
    let dir = dir.path();

    let net = MockNet::new().bind();

    // Notifies will be sent in regular intervals, with values more appropriate for tests
    // taking less time to trigger.
    let mut config = ya_payment::config::Config::from_env()?;
    config.sync_notif_backoff.run_sync_job = true;
    config.sync_notif_backoff.initial_delay = Duration::from_secs(2);
    config.sync_notif_backoff.cap = Some(Duration::from_secs(2));

    let node1 = MockNode::new(net.clone(), "node-1", dir)
        .with_identity()
        .with_payment(Some(config))
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

    let node2 = MockNode::new(net.clone(), "node-2", dir)
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

    let payment = node1.get_payment()?;
    let requestor = node1.rest_payments(&appkey_req.key)?;

    log::info!("Creating allocation...");
    let new_allocation = FakePayment::default_allocation(&agreement, BigDecimal::from(10u64))?;
    let allocation = requestor.create_allocation(&new_allocation).await?;
    log::info!("Allocation created. ({})", allocation.allocation_id);

    log::info!("================== Scenario 1 ==================");
    log::info!("Scenario: Invoice immediate acceptance");
    // Send Invoice to Requestor node.
    // Requestor is able to immediately accept.
    log::info!("Issuing invoice...");
    let invoice = FakePayment::fake_invoice(&agreement, BigDecimal::from_str("0.2")?)?;
    payment
        .gsb_public_endpoint()
        .send_as(invoice.issuer_id, SendInvoice(invoice.clone()))
        .await??;

    let mut channel = node2
        .get_fake_payment()?
        .message_channel::<AcceptInvoice>(Ok(Ack {}));

    log::info!("Accepting Invoice ({})...", invoice.invoice_id);
    requestor.get_invoice(&invoice.invoice_id).await.unwrap();
    requestor
        .simple_accept_invoice(&invoice, &allocation)
        .await
        .unwrap();

    let (_from, accept) = channel.recv().await.unwrap();
    assert_eq!(accept.invoice_id, invoice.invoice_id);
    assert_eq!(accept.acceptance.total_amount_accepted, invoice.amount);

    log::info!("================== Scenario 2 ==================");
    log::info!("Scenario: Invoice delayed acceptance");
    // Send Invoice to Requestor node.
    // Requestor attempt to accept Invoice but network is broken.
    // Acceptance will be sent later as payment sync.
    let agreement =
        FakeMarket::create_fake_agreement(appkey_req.identity, appkey_prov.identity).unwrap();
    node1.get_market()?.add_agreement(agreement.clone()).await;

    log::info!("Issuing invoice...");
    let invoice = FakePayment::fake_invoice(&agreement, BigDecimal::from_str("0.2")?)?;
    payment
        .gsb_public_endpoint()
        .send_as(invoice.issuer_id, SendInvoice(invoice.clone()))
        .await??;

    log::info!("Accepting Invoice ({})", invoice.invoice_id);
    net.break_network_for(appkey_prov.identity);

    requestor.get_invoice(&invoice.invoice_id).await.unwrap();
    requestor
        .simple_accept_invoice(&invoice, &allocation)
        .await
        .unwrap();

    // We expect that AcceptInvoice wasn't delivered.
    matches!(channel.try_recv().unwrap_err(), TryRecvError::Empty);

    let mut sync_channel = node2
        .get_fake_payment()?
        .message_channel::<PaymentSyncWithBytes>(Ok(Ack {}));
    net.enable_network_for(appkey_prov.identity);

    // We expect that PaymentSync will be delivered within 4s.
    // Looping because sync is sent from multiple identities on Requestor Node.
    loop {
        let (from, sync) = sync_channel
            .recv()
            .timeout(Some(Duration::from_secs(4)))
            .await
            .unwrap()
            .unwrap();

        if from != appkey_req.identity {
            continue;
        }

        assert_eq!(sync.invoice_accepts.len(), 1);
        assert_eq!(sync.invoice_accepts[0].invoice_id, invoice.invoice_id);
        assert_eq!(
            sync.invoice_accepts[0].acceptance.total_amount_accepted,
            invoice.amount
        );
        break;
    }

    log::info!("================== Scenario 3 ==================");
    log::info!("Scenario: Payment delayed delivery");
    // Send Invoice to Requestor node.
    // Requestor accepts Invoice, but network is broken before he sends payment confirmation.
    // Payment confirmation will be sent later as payment sync.
    let agreement =
        FakeMarket::create_fake_agreement(appkey_req.identity, appkey_prov.identity).unwrap();
    node1.get_market()?.add_agreement(agreement.clone()).await;

    log::info!("Issuing invoice...");
    let invoice = FakePayment::fake_invoice(&agreement, BigDecimal::from_str("0.2")?)?;
    payment
        .gsb_public_endpoint()
        .send_as(invoice.issuer_id, SendInvoice(invoice.clone()))
        .await??;

    log::info!("Accepting Invoice ({})", invoice.invoice_id);

    requestor.get_invoice(&invoice.invoice_id).await.unwrap();
    requestor
        .simple_accept_invoice(&invoice, &allocation)
        .await
        .unwrap();
    net.break_network_for(appkey_prov.identity);

    // Wait until Invoice will be paid on Requestor side.
    let payments = requestor
        .wait_for_invoice_payment::<Utc>(&invoice.invoice_id, Duration::from_secs(5 * 60), None)
        .await?;
    assert_eq!(payments.len(), 1);

    net.enable_network_for(appkey_prov.identity);
    // We expect that PaymentSync will be delivered within 4s.
    // Looping because sync is sent from multiple identities on Requestor Node.
    loop {
        let (from, sync) = sync_channel
            .recv()
            .timeout(Some(Duration::from_secs(4)))
            .await
            .unwrap()
            .unwrap();

        if from != appkey_req.identity {
            continue;
        }

        assert!(!sync.payments.is_empty());
        let payment = sync
            .payments
            .iter()
            .find(|p| {
                p.payment
                    .agreement_payments
                    .iter()
                    .any(|a| a.agreement_id == agreement.agreement_id)
            })
            .unwrap();

        assert_eq!(payment.payment.amount, invoice.amount);
        break;
    }

    Ok(())
}

#[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_payment_sync_fallback(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(true);

    let dir = temp_dir!("test_payment_sync_fallback")?;
    let dir = dir.into_path();

    let net = MockNet::new().bind();

    // Notifies will be sent in regular intervals, with values more appropriate for tests
    // taking less time to trigger.
    let mut config = ya_payment::config::Config::from_env()?;
    config.sync_notif_backoff.run_sync_job = true;
    config.sync_notif_backoff.initial_delay = Duration::from_secs(2);
    config.sync_notif_backoff.cap = Some(Duration::from_secs(2));

    let node1 = MockNode::new(net.clone(), "node-1", &dir)
        .with_identity()
        .with_payment(Some(config))
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

    let node2 = MockNode::new(net.clone(), "node-2", dir)
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

    let payment = node1.get_payment()?;
    let requestor = node1.rest_payments(&appkey_req.key)?;

    log::info!("Creating allocation...");
    let new_allocation = FakePayment::default_allocation(&agreement, BigDecimal::from(10u64))?;
    let allocation = requestor.create_allocation(&new_allocation).await?;
    log::info!("Allocation created. ({})", allocation.allocation_id);

    let fake_payment = node2.get_fake_payment()?;
    fake_payment.unbind_public().await;
    let mut channel = fake_payment.message_channel::<AcceptInvoice>(Ok(Ack {}));
    let mut sync_channel = fake_payment.message_channel::<PaymentSync>(Ok(Ack {}));
    let mut payment_channel = fake_payment.message_channel::<SendPayment>(Ok(Ack {}));

    log::info!("================== Scenario 1 ==================");
    log::info!("Scenario: Payment immediate delivery with fallback");
    // Send Invoice to Requestor node.
    // Requestor accepts Invoice and waits for payment confirmation.
    // Payment will be sent immediately, but PaymentSyncWithBytes has no handler on Provider, so fallback
    // mechanism should be used.
    let agreement =
        FakeMarket::create_fake_agreement(appkey_req.identity, appkey_prov.identity).unwrap();
    node1.get_market()?.add_agreement(agreement.clone()).await;

    log::info!("Issuing invoice...");
    let invoice = FakePayment::fake_invoice(&agreement, BigDecimal::from_str("0.2")?)?;
    payment
        .gsb_public_endpoint()
        .send_as(invoice.issuer_id, SendInvoice(invoice.clone()))
        .await??;

    log::info!("Accepting Invoice ({})", invoice.invoice_id);
    requestor.get_invoice(&invoice.invoice_id).await.unwrap();
    requestor
        .simple_accept_invoice(&invoice, &allocation)
        .await
        .unwrap();

    // Check if AcceptInvoice was delivered.
    let (_from, accept) = channel.recv().await.unwrap();
    assert_eq!(accept.invoice_id, invoice.invoice_id);
    assert_eq!(accept.acceptance.total_amount_accepted, invoice.amount);

    // Wait until Invoice will be paid on Requestor side.
    let payments = requestor
        .wait_for_invoice_payment::<Utc>(&invoice.invoice_id, Duration::from_secs(5 * 60), None)
        .await?;
    assert_eq!(payments.len(), 1);

    loop {
        let (from, confirmation) = payment_channel
            .recv()
            .timeout(Some(Duration::from_secs(4)))
            .await
            .unwrap()
            .unwrap();

        if from != appkey_req.identity {
            continue;
        }

        assert!(confirmation
            .payment
            .agreement_payments
            .iter()
            .any(|a| a.agreement_id == agreement.agreement_id));

        assert_eq!(confirmation.payment.amount, invoice.amount);
        break;
    }

    log::info!("================== Scenario 2 ==================");
    log::info!("Scenario: Payment delayed delivery with fallback");
    log::info!("Scenario: Invoice acceptance delayed delivery");
    // Send Invoice to Requestor node.
    // Requestor accepts Invoice and waits for payment confirmation.
    // In the meantime network is broken, so confirmation can't be immediately delivered
    // Payment will be sent later, but PaymentSyncWithBytes has no handler on Provider, so fallback
    // mechanism should be used.

    let agreement =
        FakeMarket::create_fake_agreement(appkey_req.identity, appkey_prov.identity).unwrap();
    node1.get_market()?.add_agreement(agreement.clone()).await;

    log::info!("Issuing invoice...");
    let invoice = FakePayment::fake_invoice(&agreement, BigDecimal::from_str("0.2")?)?;
    payment
        .gsb_public_endpoint()
        .send_as(invoice.issuer_id, SendInvoice(invoice.clone()))
        .await??;

    // Break network to enforce Invoice acceptance delayed delivery.
    net.break_network_for(appkey_prov.identity);

    log::info!(
        "Accepting Invoice ({})... with broken network",
        invoice.invoice_id
    );
    requestor.get_invoice(&invoice.invoice_id).await.unwrap();
    requestor
        .simple_accept_invoice(&invoice, &allocation)
        .await
        .unwrap();

    // Wait until Invoice will be paid on Requestor side.
    let payments = requestor
        .wait_for_invoice_payment::<Utc>(&invoice.invoice_id, Duration::from_secs(5 * 60), None)
        .await?;
    assert_eq!(payments.len(), 1);

    net.enable_network_for(appkey_prov.identity);
    // We expect that PaymentSync will be delivered within 4s.
    // Looping because sync is sent from multiple identities on Requestor Node.
    loop {
        let (from, sync) = sync_channel
            .recv()
            .timeout(Some(Duration::from_secs(4)))
            .await
            .unwrap()
            .unwrap();

        println!("{:?}", sync);
        if from != appkey_req.identity {
            continue;
        }

        assert!(!sync.payments.is_empty());
        let payment = sync
            .payments
            .iter()
            .find(|p| {
                p.payment
                    .agreement_payments
                    .iter()
                    .any(|a| a.agreement_id == agreement.agreement_id)
            })
            .unwrap();
        assert_eq!(payment.payment.amount, invoice.amount);

        // Check if AcceptInvoice was delivered.
        assert!(!sync.invoice_accepts.is_empty());
        assert_eq!(sync.invoice_accepts[0].invoice_id, invoice.invoice_id);
        assert_eq!(
            sync.invoice_accepts[0].acceptance.total_amount_accepted,
            invoice.amount
        );

        break;
    }

    Ok(())
}
