use bigdecimal::BigDecimal;
use chrono::Utc;
use std::time::Duration;
use ya_client::payment::{PaymentProviderApi, PaymentRequestorApi};
use ya_client::web::WebClient;
use ya_client_model::payment::{Acceptance, DocumentStatus, NewAllocation, NewInvoice};

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let client = WebClient::builder().build();
    let provider: PaymentProviderApi = client.interface()?;
    let requestor: PaymentRequestorApi = client.interface()?;
    let invoice = provider
        .issue_invoice(&NewInvoice {
            agreement_id: "agreement_id".to_string(),
            activity_ids: None,
            amount: BigDecimal::from(1u64),
            payment_due_date: Utc::now(),
        })
        .await?;
    provider.send_invoice(&invoice.invoice_id).await?;

    let allocation = requestor
        .create_allocation(&NewAllocation {
            total_amount: BigDecimal::from(10u64),
            timeout: None,
            make_deposit: false,
        })
        .await?;

    // FIXME: -1 sec is needed because timestamps have 1 sec accuracy
    let now = Utc::now() - chrono::Duration::seconds(1);
    requestor
        .accept_invoice(
            &invoice.invoice_id,
            &Acceptance {
                total_amount_accepted: invoice.amount.clone(),
                allocation_id: allocation.allocation_id,
            },
        )
        .await?;

    let timeout = Some(Duration::from_secs(300)); // Should be enough for GNT transfer
    let mut payments = provider.get_payments(Some(&now), timeout).await?;
    assert_eq!(payments.len(), 1);
    let payment = payments.pop().unwrap();
    assert_eq!(&payment.amount, &invoice.amount);

    let invoice = provider.get_invoice(&invoice.invoice_id).await?;
    assert_eq!(invoice.status, DocumentStatus::Settled);

    Ok(())
}
