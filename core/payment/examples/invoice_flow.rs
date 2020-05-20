use bigdecimal::BigDecimal;
use chrono::Utc;
use ya_client::payment::{PaymentProviderApi, PaymentRequestorApi};
use ya_client::web::{WebClient, WebInterface};
use ya_client_model::payment::{Acceptance, DocumentStatus, NewAllocation, NewInvoice};

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let provider = PaymentProviderApi::from_client(
        WebClient::builder()
            .host_port("127.0.0.1:7465/payment-api/v1/")
            .build()?,
    );
    let requestor = PaymentRequestorApi::from_client(
        WebClient::builder()
            .host_port("127.0.0.1:7465/payment-api/v1/")
            .build()?,
    );
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
    requestor
        .accept_invoice(
            &invoice.invoice_id,
            &Acceptance {
                total_amount_accepted: invoice.amount,
                allocation_id: allocation.allocation_id,
            },
        )
        .await?;

    // TODO: Listen for payment instead of sleeping
    tokio::time::delay_for(std::time::Duration::from_secs(1)).await;
    let invoice = provider.get_invoice(&invoice.invoice_id).await?;
    assert_eq!(invoice.status, DocumentStatus::Settled);

    Ok(())
}
