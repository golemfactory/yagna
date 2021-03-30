#[macro_use]
extern crate log;

use bigdecimal::BigDecimal;
use hex::ToHex;
use std::str::FromStr;
use ya_payment_driver::db::models::Network as DbNetwork;
use ya_zksync_driver::zksync::wallet as driver_wallet;
use ya_zksync_driver::zksync::{faucet, utils};
use zksync::zksync_types::{Nonce, H160, H256};
use zksync::{Network, RpcProvider, Wallet, WalletCredentials};
use zksync_eth_signer::{EthereumSigner, PrivateKeySigner};

const TOKEN: &str = "tGLM";

async fn spam_txs(private_key: H256) -> anyhow::Result<()> {
    let signer = PrivateKeySigner::new(private_key);
    let address = signer.get_address().await?;
    let addr_hex = format!("0x{}", address.encode_hex::<String>());

    info!("Funding account {}...", addr_hex);
    faucet::request_tglm(&addr_hex, DbNetwork::Rinkeby).await?;

    info!("Creating wallet {}...", addr_hex);
    let provider = RpcProvider::from_addr_and_network("http://10.30.10.98/jsrpc", Network::Rinkeby);
    let cred = WalletCredentials::from_eth_signer(address, signer, Network::Rinkeby).await?;
    let wallet = Wallet::new(provider, cred).await?;

    if wallet.is_signing_key_set().await? == false {
        info!("Unlocking account {}...", addr_hex);
        let unlock = wallet
            .start_change_pubkey()
            .fee_token(TOKEN)?
            .send()
            .await?;
        debug!("unlock={:?}", unlock);
        unlock.wait_for_commit().await?;
    }

    let amount = BigDecimal::from_str("0.1").unwrap();
    let mut nonce = driver_wallet::get_nonce(&addr_hex, DbNetwork::Rinkeby).await;
    info!("Spamming transactions {}...", addr_hex);
    while driver_wallet::account_balance(&addr_hex, DbNetwork::Rinkeby).await? > amount {
        let recipient = H160::random();
        let amount = utils::big_dec_to_big_uint(amount.clone())?;
        let amount = utils::pack_up(&amount);
        match wallet
            .start_transfer()
            .nonce(Nonce(nonce))
            .to(recipient)
            .token(TOKEN)?
            .amount(amount)
            .send()
            .await
        {
            Ok(tx_handle) => {
                info!(
                    "Sent transaction {}",
                    tx_handle.hash().encode_hex::<String>()
                );
                nonce += 1;
            }
            Err(err) => {
                error!("Error sending transaction: {:?}", err)
            }
        }
    }

    Ok(())
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let log_level = std::env::var("RUST_LOG").unwrap_or("info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();

    let private_key = H256::random();
    spam_txs(private_key).await?;
    Ok(())
}
