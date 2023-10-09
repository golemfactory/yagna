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
use std::sync::Arc;

// Workspace uses
use ya_payment_driver::cron::Cron;
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
                contract_use_direct_method: false,
                contract_use_unpacked_method: false,
                use_transfer_for_single_payment: false,
            };

            log::warn!("Loading config");
            let mut config = config::Config::load_from_str(include_str!("../config-payments.toml"))
                .expect("Default erc20next config doesn't parse");

            for (network, chain) in &mut config.chain {
                let prefix = network.to_ascii_uppercase();
                let Some(token) = &mut chain.token else { continue };
                let symbol = token.symbol.to_ascii_uppercase();

                let rpc_env = format!("{prefix}_GETH_ADDR");
                let priority_fee_env = format!("{prefix}_PRIORITY_FEE");
                let max_fee_per_gas_env = format!("{prefix}_MAX_FEE_PER_GAS");
                let token_addr_env = format!("{prefix}_{symbol}_CONTRACT_ADDRESS");
                let multi_payment_addr_env = format!("{prefix}_MULTI_PAYMENT_CONTRACT_ADDRESS");
                let confirmations = format!("ERC20_{prefix}_REQUIRED_CONFIRMATIONS");

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
                            "Valiue {max_fee} for {max_fee_per_gas_env} is not a valid devimal: {e}"
                        ),
                    }
                }
                if let Ok(addr) = env::var(&token_addr_env) {
                    match H160::from_str(&addr) {
                        Ok(parsed) => {
                            log::info!("{network} token address set to {addr}");
                            token.address = parsed;
                        }
                        Err(e) => {
                            log::warn!(
                                "Value {addr} for {token_addr_env} is not valid H160 address: {e}"
                            );
                        }
                    };
                }
                if let Ok(confirmations) = env::var(&confirmations) {
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
                } else {
                    log::debug!("{multi_payment_addr_env} not set");
                }
            }

            log::warn!("Starting payment engine: {:#?}", config);
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

            log::warn!("Payment engine started - outside task");
            let driver = Erc20NextDriver::new(pr, recv);

            driver.load_active_accounts().await;
            let driver_rc = Arc::new(driver);

            bus::bind_service(db, driver_rc.clone()).await?;

            // Start cron
            Cron::new(driver_rc);
            log::debug!("Cron started");

            log::info!("Successfully connected Erc20NextService to gsb.");
            Ok(())
        }
    }
}
