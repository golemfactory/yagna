/*
    The service that binds this payment driver into yagna via GSB.
*/

use std::{env, path::PathBuf, str::FromStr};
// External crates
use erc20_payment_lib::config;
use erc20_payment_lib::config::AdditionalOptions;
use erc20_payment_lib::misc::load_private_keys;
use erc20_payment_lib::runtime::start_payment_engine;
use ethereum_types::H160;
use std::sync::Arc;

// Workspace uses
use ya_payment_driver::{
    bus,
    dao::{init, DbExecutor},
    model::GenericError,
};

// Local uses
use crate::{driver::Erc20NextDriver, signer::IdentitySigner};

pub struct Erc20NextService;

impl Erc20NextService {
    pub async fn gsb(db: &DbExecutor, path: PathBuf) -> anyhow::Result<()> {
        log::debug!("Connecting Erc20NextService to gsb...");

        // TODO: Read and validate env
        log::debug!("Environment variables validated");

        // Init database
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
            let mut config = match config::Config::load(path.join("config-payments.toml")).await {
                Ok(config) => config,
                Err(err) => {
                    log::warn!(
                        "Failed to load config from config-payments.toml due to {err:?}, using default config"
                    );
                    config::Config::load_from_str(config_str).unwrap()
                }
            };

            let env_overrides = [
                ("goerli", "GOERLI_GETH_ADDR", "GOERLI_TGLM_CONTRACT_ADDRESS"),
                (
                    "polygon",
                    "POLYGON_GETH_ADDR",
                    "POLYGON_GLM_CONTRACT_ADDRESS",
                ),
            ];

            for env_override in env_overrides {
                log::info!("Checking overrides for {}", env_override.0);
                if let Some(goerli_conf) = config.chain.get_mut(env_override.0) {
                    if let Ok(addr) = env::var(env_override.1) {
                        log::info!("Geth addr: {addr}");

                        goerli_conf.rpc_endpoints = vec![addr];
                    }
                    if let Ok(addr) = env::var(env_override.2) {
                        log::info!("Token addr: {addr}");

                        goerli_conf.token.as_mut().unwrap().address =
                            H160::from_str(&addr).unwrap();
                    }
                }
            }

            log::warn!("Starting payment engine: {:#?}", config);
            let signer = IdentitySigner::new();

            let (sender, recv) = tokio::sync::mpsc::channel(16);

            let pr = start_payment_engine(
                &private_keys,
                &path.join("db.sqlite"),
                config,
                signer,
                None,
                Some(additional_options),
                Some(sender),
                None,
            )
            .await
            .unwrap();

            log::warn!("Payment engine started - outside task");
            let driver = Erc20NextDriver::new(pr, recv);

            driver.load_active_accounts().await;
            let driver_rc = Arc::new(driver);

            bus::bind_service(&db, driver_rc.clone()).await?;

            log::info!("Successfully connected Erc20NextService to gsb.");
            Ok(())
        }
    }
}
