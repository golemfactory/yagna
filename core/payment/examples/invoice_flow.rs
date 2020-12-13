use bigdecimal::BigDecimal;
use chrono::Utc;
use std::time::Duration;
use ya_client::payment::PaymentApi;
use ya_client::web::{WebClient, rest_api_url};
use ya_client_model::payment::{Acceptance, DocumentStatus, NewAllocation, NewInvoice, PAYMENT_API_PATH};

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let log_level = std::env::var("RUST_LOG").unwrap_or("invoice_flow=debug,info".to_owned());
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

    let invoice_date = Utc::now();

    log::info!("Issuing invoice...");
    let invoice = provider
        .issue_invoice(&NewInvoice {
            agreement_id: "agreement_id".to_string(),
            activity_ids: None,
            amount: BigDecimal::from(1.230028519070000),
            payment_due_date: Utc::now(),
        })
        .await?;
    log::debug!("invoice={:?}", invoice);
    log::info!("Invoice issued.");

    log::info!("Sending invoice...");
    provider.send_invoice(&invoice.invoice_id).await?;
    log::info!("Invoice sent.");

    let invoice_events_received = requestor
        .get_invoice_events::<Utc>(Some(&invoice_date), Some(Duration::from_secs(10)), None, None)
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
            address: None,          // Use default address (i.e. identity)
            payment_platform: None, // Use default payment platform
            total_amount: BigDecimal::from(10u64),
            timeout: None,
            make_deposit: false,
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
            Some(Duration::from_secs(10)),
            None,
            None
        )
        .await
        .unwrap();
    log::debug!("events 2: {:?}", &invoice_events_accepted);

    log::info!("Waiting for payment...");
    let timeout = Some(Duration::from_secs(300)); // Should be enough for GNT transfer
    let mut payments = provider.get_payments(Some(&now), timeout, None, None).await?;
    assert_eq!(payments.len(), 1);
    let payment = payments.pop().unwrap();
    assert_eq!(&payment.amount, &invoice.amount);
    log::info!("Payment verified correctly.");

    log::info!("Verifying invoice status...");
    let invoice = provider.get_invoice(&invoice.invoice_id).await?;
    assert_eq!(invoice.status, DocumentStatus::Settled);
    log::info!("Invoice status verified correctly.");

    let invoice_events_settled = provider
        .get_invoice_events::<Utc>(
            Some(&invoice_events_accepted.first().unwrap().event_date),
            Some(Duration::from_secs(10)),
            None,
            None
        )
        .await
        .unwrap();
    log::debug!("events 3: {:?}", &invoice_events_settled);

    Ok(())
}
