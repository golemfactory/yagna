use bigdecimal::BigDecimal;
use chrono::Utc;
use std::str::FromStr;
use std::time::Duration;
use structopt::StructOpt;
use ya_client::payment::PaymentApi;
use ya_client::web::{rest_api_url, WebClient};
use ya_client_model::payment::allocation::PaymentPlatformEnum;
use ya_client_model::payment::{Acceptance, DocumentStatus, NewAllocation, NewInvoice};

#[derive(Clone, Debug, StructOpt)]
struct Args {
    #[structopt(long)]
    app_session_id: Option<String>,
    #[structopt(long, default_value = "dummy-glm")]
    platform: String,
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let log_level =
        std::env::var("RUST_LOG").unwrap_or_else(|_| "invoice_flow=debug,info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();

    let args: Args = Args::from_args();

    // Create requestor / provider PaymentApi
    let provider_url = format!("{}provider/", rest_api_url()).parse().unwrap();
    let provider: PaymentApi = WebClient::builder()
        .api_url(provider_url)
        .build()
        .interface()?;
    let requestor_url = format!("{}requestor/", rest_api_url()).parse().unwrap();
    let requestor: PaymentApi = WebClient::builder()
        .api_url(requestor_url)
        .build()
        .interface()?;

    let invoice_date = Utc::now();

    log::info!("Issuing invoice...");
    let invoice = provider
        .issue_invoice(&NewInvoice {
            agreement_id: "agreement_id".to_string(),
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
            Some(Duration::from_secs(10)),
            None,
            args.app_session_id.clone(),
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
            payment_platform: Some(PaymentPlatformEnum::PaymentPlatformName(args.platform)),
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
            args.app_session_id.clone(),
        )
        .await
        .unwrap();
    log::debug!("events 2: {:?}", &invoice_events_accepted);

    log::info!("Waiting for payment...");
    let timeout = Some(Duration::from_secs(1000)); // Should be enough for GLM transfer
    let mut payments = provider
        .get_payments(Some(&now), timeout, None, args.app_session_id.clone())
        .await?;
    assert_eq!(payments.len(), 1);
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
            Some(Duration::from_secs(10)),
            None,
            args.app_session_id.clone(),
        )
        .await
        .unwrap();
    log::debug!("events 3: {:?}", &invoice_events_settled);

    log::info!(" 👍🏻 Example completed successfully ❤️");
    Ok(())
}
