#[macro_use]
extern crate log;

use bigdecimal::BigDecimal;
use hex::ToHex;
use std::str::FromStr;
use structopt::StructOpt;
use ya_zksync_driver::zksync::faucet;
use ya_zksync_driver::zksync::wallet as driver_wallet;
use zksync::zksync_types::H256;
use zksync::{Network, Provider, Wallet, WalletCredentials};
use zksync_eth_signer::{EthereumSigner, PrivateKeySigner};

const TOKEN: &str = "GNT";
const PRIVATE_KEY: &str = "312776bb901c426cb62238db9015c100948534dea42f9fa1591eff4beb35cc13";

#[derive(Clone, Debug, StructOpt)]
struct Args {
    #[structopt(long = "gen-key")]
    genkey: bool,

    #[structopt(long, default_value = "5.0")]
    amount: String,
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let log_level = std::env::var("RUST_LOG").unwrap_or("info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();

    let args: Args = Args::from_args();
    let private_key = if args.genkey {
        debug!("Using randomly generated key");
        H256::random()
    } else {
        debug!("Using hardcoded key");
        H256::from_str(PRIVATE_KEY).expect("Cannot decode bytes from hex-encoded PK")
    };

    let signer = PrivateKeySigner::new(private_key);
    let address = signer.get_address().await?;
    let addr_hex = format!("0x{}", address.encode_hex::<String>());
    info!("Account address {}", addr_hex);

    info!("Funding an account");
    faucet::request_ngnt(&addr_hex).await?;

    info!("Creating wallet");
    let provider = Provider::new(Network::Rinkeby);
    let cred = WalletCredentials::from_eth_signer(address, signer, Network::Rinkeby).await?;
    let wallet = Wallet::new(provider, cred).await?;

    if wallet.is_signing_key_set().await? == false {
        info!("Unlocking account");
        let unlock = wallet
            .start_change_pubkey()
            .fee_token(TOKEN)?
            .send()
            .await?;
        debug!("unlock={:?}", unlock);
        unlock.wait_for_commit().await?;
    }

    let amount: BigDecimal = args
        .amount
        .parse()
        .expect("Cannot parse 'amount' parameter to BigDecimal");

    let withdraw_handle = driver_wallet::withdraw(wallet, Some(amount)).await?;

    info!("Waiting for receipt - this takes LOOONG to complete...");
    info!(
        "Check it here: https://rinkeby.zkscan.io/explorer/accounts/{}",
        addr_hex
    );

    let tx_info = withdraw_handle.wait_for_commit().await?;
    if tx_info.success.unwrap_or(false) {
        withdraw_handle.wait_for_verify().await?;
        info!("Withdrawal succeeded!");
    } else {
        warn!("Withdraw has failed. Reason: {:?}", tx_info.fail_reason);
    }

    Ok(())
}
