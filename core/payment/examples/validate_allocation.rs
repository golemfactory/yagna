use bigdecimal::BigDecimal;
use ya_client::payment::PaymentApi;
use ya_client::web::{rest_api_url, WebClient};
use ya_client_model::payment::allocation::PaymentPlatformEnum;
use ya_client_model::payment::NewAllocation;
use ya_core_model::payment::local as pay;
use ya_service_bus::typed as bus;

async fn get_requestor_balance_and_platform() -> anyhow::Result<(BigDecimal, String)> {
    let account_list = bus::service(pay::BUS_ID)
        .call(pay::GetAccounts {})
        .await??;

    for account in account_list.into_iter() {
        if account.send {
            let status = bus::service(pay::BUS_ID)
                .call(pay::GetStatus {
                    address: account.address.clone(),
                    driver: account.driver,
                    network: Some(account.network),
                    token: Some(account.token),
                    after_timestamp: 0,
                })
                .await??;
            return Ok((status.amount, account.platform));
        }
    }

    anyhow::bail!("Requestor account not found")
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let log_level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();

    let requestor_url = format!("{}requestor/", rest_api_url()).parse().unwrap();
    let requestor: PaymentApi = WebClient::builder()
        .api_url(requestor_url)
        .build()
        .interface()?;

    let (requestor_balance, payment_platform) = get_requestor_balance_and_platform().await?;
    log::info!(
        "Requestor balance: {}, platform: {}",
        requestor_balance,
        payment_platform
    );

    if "dummy-glm" == &payment_platform {
        log::info!(
            " 🖐  Example will not work with Dummy driver as it does not validate requests 💛"
        );
        return Ok(());
    }

    log::info!("Attempting to create allocation with invalid address...");
    let result = requestor
        .create_allocation(&NewAllocation {
            address: Some("Definitely not a valid address".to_string()),
            payment_platform: Some(PaymentPlatformEnum::PaymentPlatformName(
                payment_platform.clone(),
            )),
            total_amount: BigDecimal::from(1u64),
            timeout: None,
            make_deposit: false,
        })
        .await;
    assert!(result.is_err());
    log::info!("Failed to create allocation (as expected).");

    let new_allocation = NewAllocation {
        address: None, // Use default address (i.e. identity)
        payment_platform: Some(PaymentPlatformEnum::PaymentPlatformName(payment_platform)),
        total_amount: requestor_balance,
        timeout: None,
        make_deposit: false,
    };

    log::info!(
        "Creating allocation for {} tGLM...",
        &new_allocation.total_amount
    );
    requestor.create_allocation(&new_allocation).await?;
    log::info!("Allocation created.");

    log::info!(
        "Creating another allocation for {} tGLM...",
        &new_allocation.total_amount
    );
    let allocation = requestor.create_allocation(&new_allocation).await?;
    log::info!("Allocation created.");

    log::info!(
        "Attempting to create another allocation for {} tGLM...",
        &new_allocation.total_amount
    );
    let result = requestor.create_allocation(&new_allocation).await;
    assert!(result.is_err());
    log::info!("Failed to create allocation (as expected).");

    log::info!("Releasing an allocation...");
    requestor
        .release_allocation(&allocation.allocation_id)
        .await?;
    log::info!("Allocation released.");

    log::info!(
        "Creating another allocation for {} tGLM...",
        &new_allocation.total_amount
    );
    requestor.create_allocation(&new_allocation).await?;
    log::info!("Allocation created.");

    log::info!(" 👍🏻 Example completed successfully ❤️");
    Ok(())
}
