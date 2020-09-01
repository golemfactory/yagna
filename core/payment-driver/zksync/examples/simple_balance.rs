use client::rpc_client::RpcClient;
use client::wallet::{Wallet, BalanceState};

use web3::types::{Address};
use std::str::FromStr;

#[macro_use]
extern crate log;

#[tokio::main]
async fn main() {
    let log_level = std::env::var("RUST_LOG").unwrap_or("info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();
    info!("Simple balance check example.");
    let pub_key = "7cfbf8aac6b460bf27a58e9720cf51db45b438e7";

    info!("Public key. {}", pub_key);
    let pub_address = Address::from_str(pub_key).unwrap();
    info!("Public address. {}", pub_address);
    let provider = RpcClient::new("https://rinkeby-api.zksync.io/jsrpc");

    let wallet = Wallet::from_public_address(pub_address, provider);
    let token = "GNT";
    let balance_com = wallet.get_balance(token, BalanceState::Committed).await;
    let balance_ver = wallet.get_balance(token, BalanceState::Verified).await;

    info!("balance_com: {}", balance_com);
    info!("balance_ver: {}", balance_ver);
}
