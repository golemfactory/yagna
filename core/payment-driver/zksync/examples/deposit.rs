#[macro_use]
extern crate log;

use bigdecimal::BigDecimal;
use std::str::FromStr;
use ya_payment_driver::db::models::Network as DbNetwork;
use ya_zksync_driver::zksync::wallet as driver_wallet;
use zksync::zksync_types::H256;
use zksync::{Network, RpcProvider, Wallet, WalletCredentials};
use zksync_eth_signer::{EthereumSigner, PrivateKeySigner};

const PRIVATE_KEY: &str = "e0c704b6e925c3be222337f9c94610c46b7fec95c14b8f5b9800d20ed4782670";

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let log_level = std::env::var("RUST_LOG").unwrap_or("info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();

    dotenv::dotenv().expect("Failed to read .env file");

    let private_key = H256::from_str(PRIVATE_KEY).expect("Cannot decode bytes from hex-encoded PK");
    let signer = PrivateKeySigner::new(private_key);
    let address = signer.get_address().await?;
    info!("Account address {:#x}", address);

    info!("Creating wallet");
    let provider = RpcProvider::new(Network::Rinkeby);
    let cred = WalletCredentials::from_eth_signer(address, signer, Network::Rinkeby).await?;
    let wallet = Wallet::new(provider, cred).await?;

    let one_tglm = BigDecimal::from(1);

    let deposit_tx_hash = driver_wallet::deposit(wallet, DbNetwork::Rinkeby, one_tglm).await?;
    info!(
        "Check out deposit transaction at https://rinkeby.etherscan.io/tx/{:#x}",
        deposit_tx_hash
    );

    Ok(())
}
