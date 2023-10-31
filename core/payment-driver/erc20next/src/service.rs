/*
    The service that binds this payment driver into yagna via GSB.
*/

use std::{env, path::PathBuf, str::FromStr};
// External crates
use erc20_payment_lib::config;
use erc20_payment_lib::config::{AdditionalOptions, MultiContractSettings};
use erc20_payment_lib::misc::load_private_keys;
use erc20_payment_lib::runtime::PaymentRuntime;
use ethereum_types::H160;

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
        init(db).await.map_err(GenericError::new)?;
        log::debug!("Database initialised");

        {
            let (private_keys, _public_addresses) =
                load_private_keys(&env::var("ETH_PRIVATE_KEYS").unwrap_or_default()).unwrap();
            let additional_options = AdditionalOptions {
                keep_running: true,
                generate_tx_only: false,
                skip_multi_contract_check: false,
                contract_use_direct_method: true,
                contract_use_unpacked_method: false,
                use_transfer_for_single_payment: true,
                skip_service_loop: false,
            };

            let mut config = config::Config::load_from_str(include_str!("../config-payments.toml"))
                .expect("Default erc20next config doesn't parse");

            // Load config from file if it exists giving the possibility of overwriting the default config
            if tokio::fs::try_exists(&path.join("config-payments.toml"))
                .await
                .unwrap_or(false)
            {
                log::warn!(
                    "Config file found in {}",
                    &path.join("config-payments.toml").display()
                );
                log::warn!(
                    "Format of the file may change in the future releases, use with caution!"
                );

                match config::Config::load(&path.join("config-payments.toml")).await {
                    Ok(config_from_file) => {
                        log::info!("Config file loaded successfully, overwriting default config");
                        config = config_from_file;
                    }
                    Err(err) => {
                        log::error!("Config file found but failed to load from file - using default config. Error: {err}")
                    }
                }
            } else {
                log::debug!(
                    "Config file not found in {}, using default config",
                    &path.join("config-payments.toml").display()
                );
            }

            let sendout_interval_env = "ERC20NEXT_SENDOUT_INTERVAL_SECS";
            if let Ok(sendout_interval) = env::var(sendout_interval_env) {
                match sendout_interval.parse::<u64>() {
                    Ok(sendout_interval_secs) => {
                        log::info!("erc20next gather interval set to {sendout_interval_secs}s");
                        config.engine.gather_interval = sendout_interval_secs;
                    },
                    Err(e) => log::warn!("Value {sendout_interval} for {sendout_interval_env} is not a valid integer: {e}"),
                }
            }

            for (network, chain) in &mut config.chain {
                let prefix = network.to_ascii_uppercase();
                let symbol = chain.token.symbol.to_ascii_uppercase();

                let rpc_env = format!("{prefix}_GETH_ADDR");
                let priority_fee_env = format!("{prefix}_PRIORITY_FEE");
                let max_fee_per_gas_env = format!("{prefix}_MAX_FEE_PER_GAS");
                let token_addr_env = format!("{prefix}_{symbol}_CONTRACT_ADDRESS");
                let multi_payment_addr_env = format!("{prefix}_MULTI_PAYMENT_CONTRACT_ADDRESS");
                let confirmations_env = format!("ERC20NEXT_{prefix}_REQUIRED_CONFIRMATIONS");

                if let Ok(addr) = env::var(&rpc_env) {
                    chain.rpc_endpoints = addr.split(',').map(ToOwned::to_owned).collect();
                    log::info!(
                        "{} rpc endpoints set to {:?}",
                        network,
                        &chain.rpc_endpoints
                    )
                }
                if let Ok(fee) = env::var(&priority_fee_env) {
                    match fee.parse::<f64>() {
                        Ok(fee) => {
                            log::info!("{network} priority fee set to {fee}");
                            chain.priority_fee = fee;
                        }
                        Err(e) => log::warn!(
                            "Value {fee} for {priority_fee_env} is not a valid decimal: {e}"
                        ),
                    }
                }
                if let Ok(max_fee) = env::var(&max_fee_per_gas_env) {
                    match max_fee.parse::<f64>() {
                        Ok(max_fee) => {
                            log::info!("{network} max fee per gas set to {max_fee}");
                            chain.max_fee_per_gas = max_fee;
                        }
                        Err(e) => log::warn!(
                            "Value {max_fee} for {max_fee_per_gas_env} is not a valid decimal: {e}"
                        ),
                    }
                }
                if let Ok(addr) = env::var(&token_addr_env) {
                    match H160::from_str(&addr) {
                        Ok(parsed) => {
                            log::info!("{network} token address set to {addr}");
                            chain.token.address = parsed;
                        }
                        Err(e) => {
                            log::warn!(
                                "Value {addr} for {token_addr_env} is not valid H160 address: {e}"
                            );
                        }
                    };
                }
                if let Ok(confirmations) = env::var(&confirmations_env) {
                    match confirmations.parse::<u64>() {
                        Ok(parsed) => {
                            log::info!("{network} required confirmations set to {parsed}");
                            chain.confirmation_blocks = parsed;
                        }
                        Err(e) => {
                            log::warn!(
                                "Value {confirmations} for {confirmations} is not valid u64: {e}"
                            );
                        }
                    };
                }
                if let Ok(multi_payment_addr) = env::var(&multi_payment_addr_env) {
                    match H160::from_str(&multi_payment_addr) {
                        Ok(parsed) => {
                            log::info!(
                                "{network} multi payment contract address set to {multi_payment_addr}"
                            );
                            chain.multi_contract = Some(MultiContractSettings {
                                address: parsed,
                                max_at_once: 10,
                            })
                        }
                        Err(e) => {
                            log::warn!(
                                "Value {multi_payment_addr} for {multi_payment_addr_env} is not valid H160 address: {e}"
                            );
                        }
                    };
                }
            }

            log::debug!("Starting payment engine: {:#?}", config);
            let signer = IdentitySigner::new();

            let (sender, recv) = tokio::sync::mpsc::channel(16);

            let pr = PaymentRuntime::new(
                &private_keys,
                &path.join("erc20next.sqlite"),
                config,
                signer,
                None,
                Some(additional_options),
                Some(sender),
                None,
            )
            .await
            .unwrap();

            log::debug!("Bind erc20next driver");
            let driver = Erc20NextDriver::new(pr, recv);
            driver.load_active_accounts().await;
            bus::bind_service(db, driver).await?;

            log::info!("Successfully connected Erc20NextService to gsb.");
            Ok(())
        }
    }
}
