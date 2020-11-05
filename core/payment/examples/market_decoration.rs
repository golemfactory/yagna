use bigdecimal::BigDecimal;
use ya_client::payment::PaymentRequestorApi;
use ya_client::web::WebClient;
use ya_client_model::payment::NewAllocation;
use futures::StreamExt;

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let log_level = std::env::var("RUST_LOG").unwrap_or("info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();

    let client = match std::env::var("YAGNA_APPKEY").ok() {
        Some(token) => WebClient::with_token(&token),
        None => WebClient::builder().build(),
    };
    let requestor: PaymentRequestorApi = client.interface()?;

    let accounts = requestor.get_accounts().await?;

    let allocation_ids = futures::stream::iter(accounts)
        .then(move |account| {
            let requestor = requestor.clone();
            async move {
                log::info!("Creating allocation for account {:?} ...", account);
                let allocation = requestor
                    .create_allocation(&NewAllocation {
                        address: Some(account.address.clone()),
                        payment_platform: Some(account.platform.clone()),
                        total_amount: BigDecimal::from(10u64),
                        timeout: None,
                        make_deposit: false,
                    })
                    .await.unwrap();
                log::info!("Allocation created.");
                allocation.allocation_id
            }
        })
        .collect().await;

    log::info!("Decorating demand...");
    let requestor: PaymentRequestorApi = client.interface()?;
    let decoration = requestor.decorate_demand(allocation_ids).await?;
    log::info!("Properties: {:?}", decoration.properties);
    log::info!("Constraints: {:?}", decoration.constraints);

    Ok(())
}
