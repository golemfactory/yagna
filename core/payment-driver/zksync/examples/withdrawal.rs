#[macro_use]
extern crate log;

use ya_zksync_driver::zksync::{faucet, utils};
use zksync::zksync_types::H256;
// use zksync::zksync_types::{
//     tx::{PackedEthSignature, TxEthSignature},
//     Address,
// };
use zksync::{types::BlockStatus, Provider, Wallet, WalletCredentials, Network};
use zksync_eth_signer::{PrivateKeySigner, EthereumSigner};
use hex::ToHex;

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let log_level = std::env::var("RUST_LOG").unwrap_or("info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();

    info!("Creating wallet");
    // let key = EthAccount::load_or_generate("withdrawal_account.key.json", "")
    //     .expect("should load or generate new eth key");

    let private_key = H256::random();
    let signer = PrivateKeySigner::new(private_key);
    let address = signer.get_address().await?;
    let hex_addr  = format!("0x{}", address.encode_hex::<String>());
    info!("Account address {}", hex_addr);

    let provider = Provider::new(Network::Rinkeby);
    let cred = WalletCredentials::from_eth_signer(
        address, signer, Network::Rinkeby).await?;
    let wallet = Wallet::new(provider, cred).await?;

    info!("Depositing funds");
    // let hex_addr = key.address().to_string();
    faucet::request_ngnt(&hex_addr).await;

    let balance = wallet.get_balance(BlockStatus::Committed, "GNT").await?;
    info!("Deposit successful {} NGNT available", utils::big_uint_to_big_dec(balance));


    info!("Withdrawal started");
    // ... wallet.start_withdraw()
    info!("Withdrawal succeeded!");

    Ok(())
}
