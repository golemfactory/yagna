use bigdecimal::BigDecimal;
use chrono::Utc;
use std::time::Duration;
use test_context::test_context;

use ya_client_model::payment::allocation::PaymentPlatformEnum;
use ya_client_model::payment::{Acceptance, DocumentStatus, NewAllocation, NewDebitNote};
use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::mocks::net::IMockNet;
use ya_framework_basic::{resource, temp_dir};
use ya_framework_mocks::market::FakeMarket;
use ya_framework_mocks::net::MockNet;
use ya_framework_mocks::node::MockNode;
use ya_framework_mocks::payment::{Driver, PaymentRestExt};

#[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_debit_note_flow(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("test_debit_note_flow")?;
    let dir = dir.path();

    let net = MockNet::new();
    net.bind_gsb();

    let node = MockNode::new(net, "node-1", dir)
        .with_identity()
        .with_payment()
        .with_fake_market()
        .with_fake_activity();
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

    let activity_id = node
        .get_activity()?
        .create_activity(&agreement.agreement_id)
        .await;

    let requestor = node.rest_payments(&appkey_req.key)?;
    let provider = node.rest_payments(&appkey_prov.key)?;

    node.get_payment()?
        .fund_account(Driver::Erc20, &appkey_req.identity.to_string())
        .await?;

    let payment_platform =
        PaymentPlatformEnum::PaymentPlatformName("erc20-holesky-tglm".to_string());

    let debit_note_date = Utc::now();
    let debit_note = NewDebitNote {
        activity_id: activity_id.clone(),
        total_amount_due: BigDecimal::from(1u64),
        usage_counter_vector: None,
        payment_due_date: Some(Utc::now()),
    };
    log::info!(
        "Issuing debit note (total amount due: {} GLM)...",
        &debit_note.total_amount_due
    );
    let debit_note = provider.issue_debit_note(&debit_note).await?;
    log::info!("Debit note issued.");

    log::info!("Sending debit note...");
    provider.send_debit_note(&debit_note.debit_note_id).await?;
    log::info!("Debit note sent.");

    let debit_note_events_received = requestor
        .get_debit_note_events::<Utc>(
            Some(&debit_note_date),
            Some(Duration::from_secs(10)),
            None,
            app_session_id.clone(),
        )
        .await
        .unwrap();
    log::debug!("events 1: {:?}", &debit_note_events_received);
    log::debug!(
        "DATE: {:?}",
        Some(&debit_note_events_received.first().unwrap().event_date)
    );

    log::info!("Creating allocation...");
    let allocation = requestor
        .create_allocation(&NewAllocation {
            address: None, // Use default address (i.e. identity)
            payment_platform: Some(payment_platform.clone()),
            total_amount: BigDecimal::from(10u64),
            make_deposit: false,
            deposit: None,
            timeout: None,
            extend_timeout: None,
        })
        .await?;
    log::info!("Allocation created.");

    log::debug!(
        "DEBIT_NOTES1: {:?}",
        requestor.get_debit_notes::<Utc>(None, None).await
    );
    log::debug!(
        "DEBIT_NOTES2: {:?}",
        requestor
            .get_debit_notes::<Utc>(Some(debit_note_date), None)
            .await
    );
    log::debug!(
        "DEBIT_NOTES3: {:?}",
        requestor
            .get_debit_notes::<Utc>(Some(Utc::now()), None)
            .await
    );

    log::info!("Accepting debit note...");
    let now = Utc::now();
    requestor
        .accept_debit_note(
            &debit_note.debit_note_id,
            &Acceptance {
                total_amount_accepted: debit_note.total_amount_due.clone(),
                allocation_id: allocation.allocation_id.clone(),
            },
        )
        .await?;
    log::info!("Debit note accepted.");

    log::info!("Waiting for payment...");
    let mut payments = provider
        .wait_for_payment(
            Some(&now),
            // Should be enough for GLM transfer
            Duration::from_secs(1000),
            None,
            app_session_id.clone(),
        )
        .await?;
    assert_eq!(payments.len(), 1);
    let payment = payments.pop().unwrap();
    assert_eq!(&payment.amount, &debit_note.total_amount_due);
    log::info!("Payment verified correctly.");

    log::info!("Verifying debit note status...");
    let debit_note = provider.get_debit_note(&debit_note.debit_note_id).await?;
    assert_eq!(debit_note.status, DocumentStatus::Settled);
    log::info!("Debit note status verified correctly.");

    let debit_note2 = NewDebitNote {
        activity_id: activity_id.clone(),
        total_amount_due: BigDecimal::from(2u64),
        usage_counter_vector: None,
        payment_due_date: Some(Utc::now()),
    };
    log::info!(
        "Issuing debit note (total amount due: {} GLM)...",
        debit_note2.total_amount_due
    );
    let debit_note2 = provider.issue_debit_note(&debit_note2).await?;
    log::info!("Debit note issued.");

    log::info!("Sending debit note...");
    provider.send_debit_note(&debit_note2.debit_note_id).await?;
    log::info!("Debit note sent.");

    log::info!("Accepting debit note...");
    let now = Utc::now();
    requestor
        .accept_debit_note(
            &debit_note2.debit_note_id,
            &Acceptance {
                total_amount_accepted: debit_note2.total_amount_due.clone(),
                allocation_id: allocation.allocation_id,
            },
        )
        .await?;
    log::info!("Debit note accepted.");

    log::info!("Waiting for payment...");
    let mut payments = provider
        .wait_for_payment(
            Some(&now),
            // Should be enough for GLM transfer
            Duration::from_secs(1000),
            None,
            app_session_id.clone(),
        )
        .await?;
    assert_eq!(payments.len(), 1);
    let payment = payments.pop().unwrap();
    assert_eq!(
        &payment.amount,
        &(&debit_note2.total_amount_due - &debit_note.total_amount_due)
    );
    log::info!("Payment verified correctly.");

    log::info!("Verifying debit note status...");
    let debit_note2 = provider.get_debit_note(&debit_note2.debit_note_id).await?;
    assert_eq!(debit_note2.status, DocumentStatus::Settled);
    log::info!("Debit note status verified correctly.");

    // Not implemented
    // log::debug!(
    //     "get_payments_for_debit_note1: {:?}",
    //     requestor.get_payments_for_debit_note::<Utc>(&debit_note2.debit_note_id, None, None).await
    // );
    // log::debug!(
    //     "get_payments_for_debit_note2: {:?}",
    //     requestor
    //         .get_payments_for_debit_note::<Utc>(&debit_note2.debit_note_id, Some(debit_note_date), None)
    //         .await
    // );
    // log::debug!(
    //     "get_payments_for_debit_note3: {:?}",
    //     requestor.get_payments_for_debit_note::<Utc>(&debit_note2.debit_note_id, Some(Utc::now()), None).await
    // );

    log::info!(" 👍🏻 Example completed successfully ❤️");
    Ok(())
}
