/*
    The service that binds this payment driver into yagna via GSB.
*/

// External crates
use std::sync::Arc;
use erc20_payment_lib::config;
use erc20_payment_lib::config::AdditionalOptions;
use erc20_payment_lib::runtime::start_payment_engine;

// Workspace uses
use ya_payment_driver::{
    bus,
    dao::{init, DbExecutor},
    model::GenericError,
};
use ya_service_api_interfaces::Provider;

// Local uses
use crate::driver::Erc20Driver;

pub struct Erc20Service;

impl Erc20Service {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(context: &Context) -> anyhow::Result<()> {
        log::debug!("Connecting Erc20Service to gsb...");

        // TODO: Read and validate env
        log::debug!("Environment variables validated");

        // Init database
        let db: DbExecutor = context.component();
        init(&db).await.map_err(GenericError::new)?;
        log::debug!("Database initialised");

        // Start cron
        //Cron::new(driver_rc.clone());
        log::debug!("Cron started");

        log::info!("Successfully connected Erc20Service to gsb.");

        {
            let private_keys = vec![];
            let receiver_accounts = vec![];
            let additional_options = AdditionalOptions {
                keep_running: true,
                generate_tx_only: false,
                skip_multi_contract_check: false,
            };
            let config = config::Config::load("config-payments.toml")?;

            let _pr = start_payment_engine(
                &private_keys,
                &receiver_accounts,
                "db.sqlite",
                config,
                Some(additional_options),
            ).await?;
            let driver = Erc20Driver::new(db.clone());
            driver.load_active_accounts().await;
            let driver_rc = Arc::new(driver);
            bus::bind_service(&db, driver_rc.clone()).await?;
            log::debug!("Driver loaded");
            Ok(())
        }
    }
}
