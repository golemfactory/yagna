use bigdecimal::BigDecimal;
use chrono::Utc;
use std::time::Duration;
use ya_client::payment::{PaymentProviderApi, PaymentRequestorApi};
use ya_client::web::WebClient;
use ya_client_model::payment::{
    Acceptance, DocumentStatus, EventType, NewAllocation, NewDebitNote, NewInvoice,
};
use ya_core_model::payment::local as pay;
use ya_service_bus::typed as bus;

async fn assert_requested_amount(
    payer_addr: &str,
    payee_addr: &str,
    payment_platform: &str,
    amount: &BigDecimal,
) -> anyhow::Result<()> {
    let payer_status = bus::service(pay::BUS_ID)
        .call(pay::GetStatus {
            platform: payment_platform.to_string(),
            address: payer_addr.to_string(),
        })
        .await??;
    assert_eq!(&payer_status.outgoing.requested, amount);

    let payee_status = bus::service(pay::BUS_ID)
        .call(pay::GetStatus {
            platform: payment_platform.to_string(),
            address: payee_addr.to_string(),
        })
        .await??;
    assert_eq!(&payee_status.incoming.requested, amount);
    Ok(())
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let log_level = std::env::var("RUST_LOG").unwrap_or("info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();

    let client = WebClient::builder().build();
    let provider: PaymentProviderApi = client.interface()?;
    let requestor: PaymentRequestorApi = client.interface()?;

    let debit_note = NewDebitNote {
        activity_id: "activity1".to_string(),
        total_amount_due: BigDecimal::from(1u64),
        usage_counter_vector: None,
        payment_due_date: Some(Utc::now()),
    };
    log::info!(
        "Issuing debit note for activity 1 (total amount due: {} NGNT)...",
        &debit_note.total_amount_due
    );
    let debit_note = provider.issue_debit_note(&debit_note).await?;
    log::info!("Debit note issued.");

    log::info!("Sending debit note...");
    provider.send_debit_note(&debit_note.debit_note_id).await?;
    log::info!("Debit note sent.");

    let debit_note2 = NewDebitNote {
        activity_id: "activity2".to_string(),
        total_amount_due: BigDecimal::from(1u64),
        usage_counter_vector: None,
        payment_due_date: Some(Utc::now()),
    };
    log::info!(
        "Issuing debit note for activity 2 (total amount due: {} NGNT)...",
        debit_note2.total_amount_due
    );
    let debit_note2 = provider.issue_debit_note(&debit_note2).await?;
    log::info!("Debit note issued.");

    log::info!("Sending debit note...");
    provider.send_debit_note(&debit_note2.debit_note_id).await?;
    log::info!("Debit note sent.");

    let payer_addr = debit_note.payer_addr;
    let payee_addr = debit_note.payee_addr;
    let payment_platform = debit_note.payment_platform;
    let amount = &debit_note.total_amount_due + &debit_note2.total_amount_due;

    assert_requested_amount(&payer_addr, &payee_addr, &payment_platform, &amount).await?;

    let invoice = NewInvoice {
        agreement_id: "agreement_id".to_string(),
        activity_ids: None,
        amount: BigDecimal::from(3u64),
        payment_due_date: Utc::now(),
    };
    log::info!("Issuing invoice (amount: {} NGNT)...", &invoice.amount);
    let invoice = provider.issue_invoice(&invoice).await?;
    log::info!("Invoice issued.");

    log::info!("Sending invoice...");
    provider.send_invoice(&invoice.invoice_id).await?;
    log::info!("Invoice sent.");

    assert_requested_amount(&payer_addr, &payee_addr, &payment_platform, &invoice.amount).await?;

    log::info!("Cancelling invoice...");
    let now = Utc::now();
    provider.cancel_invoice(&invoice.invoice_id).await?;
    log::info!("Invoice cancelled.");

    log::info!("Listening for invoice cancelled event...");
    let mut events = requestor
        .get_invoice_events(Some(&now), Some(Duration::from_secs(5)))
        .await?;
    assert_eq!(events.len(), 1);
    let event = events.pop().unwrap();
    assert_eq!(&event.invoice_id, &invoice.invoice_id);
    assert_eq!(&event.event_type, &EventType::Cancelled);
    log::info!("Event received and verified.");

    log::info!("Verifying invoice status...");
    let invoice = provider.get_invoice(&invoice.invoice_id).await?;
    assert_eq!(invoice.status, DocumentStatus::Cancelled);
    let invoice = requestor.get_invoice(&invoice.invoice_id).await?;
    assert_eq!(invoice.status, DocumentStatus::Cancelled);
    log::info!("Invoice status verified correctly.");

    assert_requested_amount(&payer_addr, &payee_addr, &payment_platform, &amount).await?;

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

    log::info!("Attempting to accept cancelled invoice...");
    let acceptance = Acceptance {
        total_amount_accepted: invoice.amount.clone(),
        allocation_id: allocation.allocation_id,
    };
    let accept_result = requestor
        .accept_invoice(&invoice.invoice_id, &acceptance)
        .await;
    accept_result.unwrap_err();
    log::info!("Failed to accept cancelled invoice.");

    let invoice = NewInvoice {
        agreement_id: "agreement_id".to_string(),
        activity_ids: None,
        amount: BigDecimal::from(3u64),
        payment_due_date: Utc::now(),
    };
    log::info!("Issuing invoice (amount: {} NGNT)...", &invoice.amount);
    let invoice = provider.issue_invoice(&invoice).await?;
    log::info!("Invoice issued.");

    log::info!("Sending invoice...");
    provider.send_invoice(&invoice.invoice_id).await?;
    log::info!("Invoice sent.");

    log::info!("Accepting invoice...");
    requestor
        .accept_invoice(&invoice.invoice_id, &acceptance)
        .await?;
    log::info!("Invoice accepted.");

    log::info!("Attempting to cancel accepted invoice...");
    let cancel_result = provider.cancel_invoice(&invoice.invoice_id).await;
    cancel_result.unwrap_err();
    log::info!("Failed to cancel accepted invoice.");

    Ok(())
}
