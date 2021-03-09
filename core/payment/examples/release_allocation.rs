use bigdecimal::BigDecimal;
use chrono::Utc;
use ya_client::payment::PaymentApi;
use ya_client::web::{rest_api_url, WebClient};
use ya_client_model::payment::{Acceptance, NewAllocation, NewInvoice};

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let log_level = std::env::var("RUST_LOG").unwrap_or("info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();

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

    log::info!("Creating allocation...");
    let accounts = requestor.get_requestor_accounts().await?;
    let account = accounts.first().expect("No account available");
    let allocation = requestor
        .create_allocation(&NewAllocation {
            address: Some(account.address.clone()),
            payment_platform: Some(account.platform.clone()),
            total_amount: BigDecimal::from(10u64),
            timeout: None,
            make_deposit: false,
        })
        .await?;
    log::info!("Allocation created.");

    log::info!("Verifying allocation...");
    let allocations = requestor.get_allocations::<Utc>(None, None).await?;
    assert_eq!(allocations.len(), 1);
    assert_eq!(allocations[0], allocation);
    let allocation1 = requestor.get_allocation(&allocation.allocation_id).await?;
    assert_eq!(allocation1, allocation);
    log::info!("Done.");

    log::info!("Releasing allocation...");
    requestor
        .release_allocation(&allocation.allocation_id)
        .await?;
    log::info!("Allocation released.");

    log::info!("Verifying allocation removal...");
    let allocations = requestor.get_allocations::<Utc>(None, None).await?;
    assert_eq!(allocations.len(), 0);
    let result = requestor.get_allocation(&allocation.allocation_id).await;
    assert!(result.is_err());
    log::info!("Done.");

    log::info!("Issuing invoice...");
    let invoice = provider
        .issue_invoice(&NewInvoice {
            agreement_id: "agreement_id".to_string(),
            activity_ids: None,
            amount: BigDecimal::from(1u64),
            payment_due_date: Utc::now(),
        })
        .await?;
    log::info!("Invoice issued.");

    log::info!("Sending invoice...");
    provider.send_invoice(&invoice.invoice_id).await?;
    log::info!("Invoice sent.");

    log::info!("Attempting to accept invoice...");
    let result = requestor
        .accept_invoice(
            &invoice.invoice_id,
            &Acceptance {
                total_amount_accepted: invoice.amount.clone(),
                allocation_id: allocation.allocation_id,
            },
        )
        .await;
    assert!(result.is_err());
    log::info!("Failed to accept invoice (as expected).");

    log::info!("Creating another allocation...");
    let allocation = requestor
        .create_allocation(&NewAllocation {
            address: Some(account.address.clone()),
            payment_platform: Some(account.platform.clone()),
            total_amount: BigDecimal::from(10u64),
            timeout: None,
            make_deposit: false,
        })
        .await?;
    log::info!("Allocation created.");

    log::info!("Accepting invoice...");
    requestor
        .accept_invoice(
            &invoice.invoice_id,
            &Acceptance {
                total_amount_accepted: invoice.amount.clone(),
                allocation_id: allocation.allocation_id.clone(),
            },
        )
        .await?;
    log::info!("Invoice accepted.");

    log::info!("Releasing allocation...");
    requestor
        .release_allocation(&allocation.allocation_id)
        .await?;
    log::info!("Allocation released.");

    log::info!("Verifying allocation removal...");
    let allocations = requestor.get_allocations::<Utc>(None, None).await?;
    assert_eq!(allocations.len(), 0);
    let result = requestor.get_allocation(&allocation.allocation_id).await;
    assert!(result.is_err());
    log::info!("Done.");

    log::info!(" 👍🏻 Example completed successfully ❤️");
    Ok(())
}
