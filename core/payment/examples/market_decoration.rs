use bigdecimal::BigDecimal;
use futures::StreamExt;
use ya_client::payment::PaymentApi;
use ya_client::web::{rest_api_url, WebClient};
use ya_client_model::payment::allocation::PaymentPlatformEnum;
use ya_client_model::payment::NewAllocation;

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let log_level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();

    let requestor_url = format!("{}requestor/", rest_api_url()).parse().unwrap();
    let client = match std::env::var("YAGNA_APPKEY").ok() {
        Some(token) => WebClient::builder()
            .api_url(requestor_url)
            .auth_token(&token)
            .build(),
        None => WebClient::builder().api_url(requestor_url).build(),
    };
    let requestor: PaymentApi = client.interface()?;

    let accounts = requestor.get_requestor_accounts().await?;

    let allocation_ids = futures::stream::iter(accounts)
        .then(move |account| {
            let requestor = requestor.clone();
            async move {
                log::info!("Creating allocation for account {:?} ...", account);
                let allocation = requestor
                    .create_allocation(&NewAllocation {
                        address: Some(account.address.clone()),
                        payment_platform: Some(PaymentPlatformEnum::PaymentPlatformName(
                            account.platform.clone(),
                        )),
                        total_amount: BigDecimal::from(10u64),
                        timeout: None,
                        make_deposit: false,
                    })
                    .await
                    .unwrap();
                log::info!("Allocation created.");
                allocation.allocation_id
            }
        })
        .collect()
        .await;

    log::info!("Decorating demand...");
    let requestor: PaymentApi = client.interface()?;
    let decoration = requestor.get_demand_decorations(allocation_ids).await?;
    log::info!("Properties: {:?}", decoration.properties);
    log::info!("Constraints: {:?}", decoration.constraints);

    log::info!(" 👍🏻 Example completed successfully ❤️");
    Ok(())
}
