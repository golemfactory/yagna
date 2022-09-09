/*
    Top up new accounts from the rinkeby erc20 faucet and wait for the funds to arive.
*/

// External crates
use bigdecimal::{BigDecimal, FromPrimitive};
use chrono::{Duration, Utc};
use lazy_static::lazy_static;
use std::{env, time};
use tokio::time::sleep;
use web3::types::{H160, U256};

// Workspace uses
use ya_payment_driver::{db::models::Network, model::GenericError, utils};
use ya_utils_networking::resolver;

// Local uses
use crate::dao::Erc20Dao;
use crate::erc20::{ethereum, wallet};

const DEFAULT_FAUCET_SRV_PREFIX: &str = "_eth-faucet._tcp";
const DEFAULT_ETH_FAUCET_HOST: &str = "faucet.testnet.golem.network";
const FAUCET_ADDR_ENVAR: &str = "ETH_FAUCET_ADDRESS";
const MAX_FAUCET_REQUESTS: u32 = 6;

lazy_static! {
    static ref MIN_GLM_BALANCE: U256 = utils::big_dec_to_u256(&BigDecimal::from(50));
    static ref MIN_ETH_BALANCE: U256 =
        utils::big_dec_to_u256(&BigDecimal::from_f64(0.005).unwrap());
    static ref MAX_WAIT: Duration = Duration::minutes(1);
}

pub async fn request_glm(
    dao: &Erc20Dao,
    address: H160,
    network: Network,
) -> Result<(), GenericError> {
    let str_addr = format!("0x{:x}", address);
    let balance = ethereum::get_balance(address, network).await?;
    if balance >= *MIN_ETH_BALANCE {
        log::info!("Enough tETH balance.");
    } else {
        log::info!(
            "Requesting tETH from erc20 faucet... address = {}",
            &str_addr
        );

        for i in 0..MAX_FAUCET_REQUESTS {
            match faucet_donate(address, network).await {
                Ok(()) => break,
                Err(e) => {
                    // Do not warn nor sleep at the last try.
                    if i >= MAX_FAUCET_REQUESTS - 1 {
                        log::error!(
                            "Failed to request tGLM from Faucet, tried {} times.: {:?}",
                            MAX_FAUCET_REQUESTS,
                            e
                        );
                        return Err(e);
                    } else {
                        log::warn!(
                            "Retrying ({}/{}) to request tGLM from Faucet after failure: {:?}",
                            i + 1,
                            MAX_FAUCET_REQUESTS,
                            e
                        );
                        sleep(time::Duration::from_secs(10)).await;
                    }
                }
            }
        }
        wait_for_eth(address, network).await?;
    }
    let glm_balance = ethereum::get_glm_balance(address, network).await?;

    if glm_balance >= *MIN_GLM_BALANCE {
        log::info!("Enough tGLM balance.");
        return Ok(());
    }
    let pending = dao.get_pending_faucet_txs(&str_addr, network).await;
    //TODO Rafał
    if let Some(_tx) = pending.into_iter().next() {
        log::info!("Already pending a mint transactin.");
        return Ok(());
    }
    log::info!(
        "Requesting tGLM from erc20 faucet... address = {}",
        &str_addr
    );

    let nonce = wallet::get_next_nonce(dao, address, network).await?;
    let db_tx = ethereum::sign_faucet_tx(address, network, nonce).await?;
    // After inserting into the database, the tx will get send by the send_payments job
    dao.insert_raw_transaction(db_tx).await;

    // Wait for tx to get mined:
    // - send_payments job runs every 10 seconds
    // - blocks are mined every 15 seconds
    sleep(time::Duration::from_secs(10)).await;

    wait_for_glm(address, network).await?;

    Ok(())
}

async fn wait_for_eth(address: H160, network: Network) -> Result<(), GenericError> {
    log::info!("Waiting for tETH from faucet...");
    let wait_until = Utc::now() + *MAX_WAIT;
    while Utc::now() < wait_until {
        if ethereum::get_balance(address, network).await? >= *MIN_ETH_BALANCE {
            log::info!("Received tETH from faucet.");
            return Ok(());
        }
        sleep(time::Duration::from_secs(3)).await;
    }
    let msg = "Waiting for tETH timed out.";
    log::error!("{}", msg);
    Err(GenericError::new(msg))
}

async fn wait_for_glm(address: H160, network: Network) -> Result<(), GenericError> {
    log::info!("Waiting for tGLM from faucet...");
    let wait_until = Utc::now() + *MAX_WAIT;
    while Utc::now() < wait_until {
        if ethereum::get_glm_balance(address, network).await? >= *MIN_GLM_BALANCE {
            log::info!("Received tGLM from faucet.");
            return Ok(());
        }
        sleep(time::Duration::from_secs(3)).await;
    }
    let msg = "Waiting for tGLM timed out.";
    log::error!("{}", msg);
    Err(GenericError::new(msg))
}

async fn faucet_donate(address: H160, _network: Network) -> Result<(), GenericError> {
    // TODO: Reduce timeout to 20-30 seconds when transfer is used.
    let client = awc::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .finish();
    let faucet_url = resolve_faucet_url().await?;
    let request_url = format!("{}/0x{:x}", faucet_url, address);
    let request_url = resolver::try_resolve_dns_record(&request_url).await;
    debug!("Faucet request url: {}", request_url);
    let response = client
        .get(request_url)
        .send()
        .await
        .map_err(GenericError::new)?
        .body()
        .await
        .map_err(GenericError::new)?;
    let response = String::from_utf8_lossy(response.as_ref());
    log::debug!("Funds requested. Response = {}", response);
    // TODO: Verify tx hash
    Ok(())
}

async fn resolve_faucet_url() -> Result<String, GenericError> {
    match env::var(FAUCET_ADDR_ENVAR) {
        Ok(addr) => Ok(addr),
        _ => {
            let faucet_host = resolver::resolve_yagna_srv_record(DEFAULT_FAUCET_SRV_PREFIX)
                .await
                .unwrap_or_else(|_| DEFAULT_ETH_FAUCET_HOST.to_string());

            Ok(format!("http://{}:4000/donate", faucet_host))
        }
    }
}
