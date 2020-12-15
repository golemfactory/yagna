use bigdecimal::BigDecimal;
use chrono::Utc;
use std::time::Duration;
use ya_client::payment::PaymentApi;
use ya_client::web::{rest_api_url, WebClient};
use ya_client_model::payment::{
    Acceptance, DocumentStatus, NewAllocation, NewDebitNote, PAYMENT_API_PATH,
};

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let log_level = std::env::var("RUST_LOG").unwrap_or("debit_note_flow=debug,info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();

    // Create requestor / provider PaymentApi
    let rest_api_url = format!("{}{}", rest_api_url(), PAYMENT_API_PATH);
    let provider_url = format!("{}provider/", &rest_api_url);
    std::env::set_var("YAGNA_PAYMENT_URL", provider_url);
    let provider: PaymentApi = WebClient::builder().build().interface()?;
    let requestor_url = format!("{}requestor/", &rest_api_url);
    std::env::set_var("YAGNA_PAYMENT_URL", requestor_url);
    let requestor: PaymentApi = WebClient::builder().build().interface()?;

    let debit_note_date = Utc::now();

    let debit_note = NewDebitNote {
        activity_id: "activity_id".to_string(),
        total_amount_due: BigDecimal::from(1u64),
        usage_counter_vector: None,
        payment_due_date: Some(Utc::now()),
    };
    log::info!(
        "Issuing debit note (total amount due: {} NGNT)...",
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
            None,
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
            address: None,          // Use default address (i.e. identity)
            payment_platform: None, // Use default payment platform
            total_amount: BigDecimal::from(10u64),
            timeout: None,
            make_deposit: false,
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
    let timeout = Some(Duration::from_secs(300)); // Should be enough for GNT transfer
    let mut payments = provider
        .get_payments(Some(&now), timeout, None, None)
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
        activity_id: "activity_id".to_string(),
        total_amount_due: BigDecimal::from(2u64),
        usage_counter_vector: None,
        payment_due_date: Some(Utc::now()),
    };
    log::info!(
        "Issuing debit note (total amount due: {} NGNT)...",
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
    let timeout = Some(Duration::from_secs(300)); // Should be enough for GNT transfer
    let mut payments = provider
        .get_payments(Some(&now), timeout, None, None)
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

    Ok(())
}
