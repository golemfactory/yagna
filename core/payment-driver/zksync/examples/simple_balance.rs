use zksync::zksync_types::Address;
use zksync::Network;
use zksync::{provider::Provider, RpcProvider};

use std::str::FromStr;

#[macro_use]
extern crate log;

#[tokio::main]
async fn main() {
    let log_level = std::env::var("RUST_LOG").unwrap_or("info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();
    info!("Simple balance check example.");
    let pub_key = "bf55a824c3114b07899e63870917da1bc01bcd06";

    info!("Public key. {}", pub_key);
    let pub_address = Address::from_str(pub_key).unwrap();
    info!("Public address. {}", pub_address);
    let provider = RpcProvider::new(Network::Rinkeby);

    let acc_info = provider.account_info(pub_address).await.unwrap();
    debug!("{:?}", acc_info);
    let token = "tGLM";
    let balance_com = acc_info
        .committed
        .balances
        .get(&token as &str)
        .map(|x| x.0.clone())
        .unwrap_or_default();
    let balance_ver = acc_info
        .verified
        .balances
        .get(&token as &str)
        .map(|x| x.0.clone())
        .unwrap_or_default();

    info!("balance_com: {}", balance_com);
    info!("balance_ver: {}", balance_ver);
}
