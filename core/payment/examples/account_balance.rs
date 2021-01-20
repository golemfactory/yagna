use ya_core_model::payment::local as pay;
use ya_service_bus::typed as bus;

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let log_level = std::env::var("RUST_LOG").unwrap_or("info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();

    let account_list = bus::service(pay::BUS_ID)
        .call(pay::GetAccounts {})
        .await??;
    log::debug!("account_list: {:?}", account_list);

    for account in account_list.into_iter() {
        let payer_status = bus::service(pay::BUS_ID)
            .call(pay::GetStatus {
                address: account.address.to_string(),
                platform: Some(account.platform.to_string()),
                driver: None,
                network: None,
                token: None,
            })
            .await??;

        log::info!("Address: {:?}", account.address);
        log::info!("Balance: {:?}", payer_status.amount);
        log::debug!("payer_status: {:?}", payer_status);
    }
    Ok(())
}
