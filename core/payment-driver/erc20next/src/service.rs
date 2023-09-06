/*
    The service that binds this payment driver into yagna via GSB.
*/

use std::{env, path::Path};
// External crates
use erc20_payment_lib::config;
use erc20_payment_lib::config::AdditionalOptions;
use erc20_payment_lib::misc::load_private_keys;
use erc20_payment_lib::runtime::PaymentRuntime;
use std::sync::Arc;

// Workspace uses
use ya_payment_driver::{
    bus,
    dao::{init, DbExecutor},
    model::GenericError,
};
use ya_service_api_interfaces::Provider;

// Local uses
use crate::{driver::Erc20NextDriver, signer::IdentitySigner};

pub struct Erc20NextService;

impl Erc20NextService {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(context: &Context) -> anyhow::Result<()> {
        log::debug!("Connecting Erc20NextService to gsb...");

        // TODO: Read and validate env
        log::debug!("Environment variables validated");

        // Init database
        let db: DbExecutor = context.component();
        init(&db).await.map_err(GenericError::new)?;
        log::debug!("Database initialised");

        // Start cron
        //Cron::new(driver_rc.clone());
        log::debug!("Cron started");

        {
            let (private_keys, _public_addresses) =
                load_private_keys(&env::var("ETH_PRIVATE_KEYS").unwrap_or_default()).unwrap();
            let additional_options = AdditionalOptions {
                keep_running: true,
                generate_tx_only: false,
                skip_multi_contract_check: false,
                contract_use_direct_method: false,
                contract_use_unpacked_method: false,
            };
            log::warn!("Loading config");
            let config_str = include_str!("../config-payments.toml");
            let config = match config::Config::load("config-payments.toml").await {
                Ok(config) => config,
                Err(err) => {
                    log::warn!(
                        "Failed to load config from config-payments.toml due to {err:?}, using default config"
                    );
                    config::Config::load_from_str(config_str).unwrap()
                }
            };

            log::warn!("Starting payment engine: {:#?}", config);
            let signer = IdentitySigner::new();
            let pr = PaymentRuntime::new(
                &private_keys,
                Path::new("db.sqlite"),
                config,
                signer,
                None,
                Some(additional_options),
                None,
                None,
            )
            .await
            .unwrap();
            log::warn!("Payment engine started - outside task");
            let driver = Erc20NextDriver::new(pr);
            driver.load_active_accounts().await;
            let driver_rc = Arc::new(driver);
            bus::bind_service(&db, driver_rc.clone()).await?;

            log::info!("Successfully connected Erc20NextService to gsb.");
            Ok(())
        }
    }
}
