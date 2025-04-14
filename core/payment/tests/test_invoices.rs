use bigdecimal::BigDecimal;
use chrono::Utc;
use std::str::FromStr;
use std::time::Duration;
use test_context::test_context;

use ya_client_model::payment::allocation::PaymentPlatformEnum;
use ya_client_model::payment::{Acceptance, DocumentStatus, NewAllocation, NewInvoice};
use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::{resource, temp_dir};
use ya_framework_mocks::market::FakeMarket;
use ya_framework_mocks::net::MockNet;
use ya_framework_mocks::node::MockNode;
use ya_framework_mocks::payment::{Driver, PaymentRestExt};

#[cfg_attr(not(feature = "system-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_invoice_flow(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("test_invoice_flow")?;
    let dir = dir.path();

    let net = MockNet::new().bind();

    let node = MockNode::new(net, "node-1", dir)
        .with_identity()
        .with_payment(None)
        .with_fake_market();
    node.bind_gsb().await?;
    node.start_server(ctx).await?;

    let appkey_prov = node.get_identity()?.create_identity_key("provider").await?;
    let appkey_req = node
        .get_identity()?
        .create_from_private_key(&resource!("ci-requestor-1.key.priv"))
        .await?;

    let app_session_id = Some("app_session_id".to_string());
    let mut agreement =
        FakeMarket::create_fake_agreement(appkey_req.identity, appkey_prov.identity).unwrap();
    agreement.app_session_id = app_session_id.clone();
    node.get_market()?.add_agreement(agreement.clone()).await;

    let requestor = node.rest_payments(&appkey_req.key)?;
    let provider = node.rest_payments(&appkey_prov.key)?;

    node.get_payment()?
        .fund_account(Driver::Erc20, &appkey_req.identity.to_string())
        .await?;

    let payment_platform =
        PaymentPlatformEnum::PaymentPlatformName("erc20-holesky-tglm".to_string());

    let invoice_date = Utc::now();

    log::info!("Issuing invoice...");
    let invoice = provider
        .issue_invoice(&NewInvoice {
            agreement_id: agreement.agreement_id.to_string(),
            activity_ids: None,
            amount: BigDecimal::from_str("1.230028519070000")?,
            payment_due_date: Utc::now(),
        })
        .await?;
    log::debug!("invoice={:?}", invoice);
    log::info!("Invoice issued.");

    log::info!("Sending invoice...");
    provider.send_invoice(&invoice.invoice_id).await?;
    log::info!("Invoice sent.");

    let invoice_events_received = requestor
        .get_invoice_events::<Utc>(
            Some(&invoice_date),
            Some(Duration::from_secs(1000)),
            None,
            app_session_id.clone(),
        )
        .await
        .unwrap();
    log::debug!("events 1: {:?}", &invoice_events_received);
    log::debug!(
        "DATE: {:?}",
        Some(&invoice_events_received.first().unwrap().event_date)
    );

    log::info!("Creating allocation...");
    let allocation = requestor
        .create_allocation(&NewAllocation {
            address: None, // Use default address (i.e. identity)
            payment_platform: Some(payment_platform.clone()),
            total_amount: BigDecimal::from(10u64),
            timeout: None,
            make_deposit: false,
            deposit: None,
            extend_timeout: None,
        })
        .await?;
    log::debug!("allocation={:?}", allocation);
    log::info!("Allocation created.");

    log::debug!(
        "INVOICES1: {:?}",
        requestor.get_invoices::<Utc>(None, None).await
    );
    log::debug!(
        "INVOICES2: {:?}",
        requestor
            .get_invoices::<Utc>(Some(invoice_date), None)
            .await
    );
    log::debug!(
        "INVOICES3: {:?}",
        requestor.get_invoices::<Utc>(Some(Utc::now()), None).await
    );

    log::info!("Accepting invoice...");
    let now = Utc::now();
    requestor
        .accept_invoice(
            &invoice.invoice_id,
            &Acceptance {
                total_amount_accepted: invoice.amount.clone(),
                allocation_id: allocation.allocation_id,
            },
        )
        .await?;
    log::info!("Invoice accepted.");

    let invoice_events_accepted = provider
        .get_invoice_events::<Utc>(
            Some(&invoice_events_received.first().unwrap().event_date),
            Some(Duration::from_secs(1000)),
            None,
            app_session_id.clone(),
        )
        .await
        .unwrap();
    log::debug!("events 2: {:?}", &invoice_events_accepted);

    log::info!("Waiting for payment on requestor...");
    let mut payments = requestor
        .wait_for_payment(
            Some(&now),
            // Should be enough for GLM transfer
            Duration::from_secs(5 * 60),
            None,
            app_session_id.clone(),
        )
        .await?;
    assert_eq!(payments.len(), 1);
    let payment = payments.pop().unwrap();
    assert!(payment.amount >= invoice.amount);

    log::info!("Waiting for payment on provider...");
    let mut payments = provider
        .wait_for_payment(
            Some(&now),
            Duration::from_secs(60),
            None,
            app_session_id.clone(),
        )
        .await?;
    let signed_payments = provider
        .get_signed_payments(Some(&now), None, None, app_session_id.clone())
        .await?;

    assert_eq!(payments.len(), 1);
    assert_eq!(signed_payments.len(), 1);

    let payment = payments.pop().unwrap();
    assert!(payment.amount >= invoice.amount);

    log::info!("Payment verified correctly.");

    log::info!("Verifying invoice status...");
    let invoice = provider.get_invoice(&invoice.invoice_id).await?;
    assert_eq!(invoice.status, DocumentStatus::Settled);
    log::info!("Invoice status verified correctly.");

    let invoice_events_settled = provider
        .get_invoice_events::<Utc>(
            Some(&invoice_events_accepted.first().unwrap().event_date),
            Some(Duration::from_secs(1000)),
            None,
            app_session_id.clone(),
        )
        .await
        .unwrap();
    log::debug!("events 3: {:?}", &invoice_events_settled);

    log::info!(" 👍🏻 Example completed successfully ❤️");
    Ok(())
}
