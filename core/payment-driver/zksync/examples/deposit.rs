#[macro_use]
extern crate log;

use hex::ToHex;
use std::str::FromStr;
use zksync::zksync_types::{H256, U256};
use zksync::{Network, RpcProvider, Wallet, WalletCredentials};
use zksync_eth_signer::{EthereumSigner, PrivateKeySigner};


const TOKEN: &str = "tGLM";
const PRIVATE_KEY: &str = "e0c704b6e925c3be222337f9c94610c46b7fec95c14b8f5b9800d20ed4782670";

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let log_level = std::env::var("RUST_LOG").unwrap_or("info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();

    dotenv::dotenv().expect("Failed to read .env file");

    let private_key = H256::from_str(PRIVATE_KEY)
        .expect("Cannot decode bytes from hex-encoded PK");
    let signer = PrivateKeySigner::new(private_key);
    let address = signer.get_address().await?;
    let addr_hex = format!("0x{}", address.encode_hex::<String>());
    info!("Account address {}", addr_hex);

    info!("Creating wallet");
    let provider = RpcProvider::new(Network::Rinkeby);
    let cred = WalletCredentials::from_eth_signer(address, signer, Network::Rinkeby).await?;
    let wallet = Wallet::new(provider, cred).await?;

    let eth_node_url = std::env::var("ERC20_RINKEBY_GETH_ADDR").expect("ETH node url has to be provided");
    let ethereum = wallet.ethereum(eth_node_url).await?;

    let one_tglm = U256::from(10).pow(18.into());
    let deposit_tx_hash = ethereum.deposit(
             TOKEN,
             one_tglm,
        wallet.address()
        ).await?;

    info!("Check out deposit transaction at\nhttps://rinkeby.etherscan.io/tx/0x{}", hex::encode(deposit_tx_hash.as_fixed_bytes()));

    Ok(())
}