#[macro_use]
extern crate log;

use bigdecimal::BigDecimal;
use hex::ToHex;
use ya_zksync_driver::zksync::{faucet, utils};
use zksync::zksync_types::{TxFeeTypes, H256};
use zksync::{types::BlockStatus, Network, Provider, Wallet, WalletCredentials};
use zksync_eth_signer::{EthereumSigner, PrivateKeySigner};

use std::cmp::Ordering;
use structopt::StructOpt;

const TOKEN: &str = "GNT";
const PRIVATE_KEY: &str = "312776bb901c426cb62238db9015c100948534dea42f9fa1591eff4beb35cc13";

#[derive(Clone, Debug, StructOpt)]
struct Args {
    #[structopt(long = "gen-key")]
    genkey: bool,

    #[structopt(long, default_value = "5.0")]
    amount: f64,
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
        let mut bytes = [0u8; 32];
        hex::decode_to_slice(PRIVATE_KEY, &mut bytes)
            .expect("Cannot decode bytes from hex-encoded PK");
        H256::from(bytes)
    };

    let pk_hex: String = private_key.encode_hex();
    let signer = PrivateKeySigner::new(private_key);
    let address = signer.get_address().await?;
    let addr_hex = format!("0x{}", address.encode_hex::<String>());
    info!("Private key: {}\nAccount address {}", pk_hex, addr_hex);

    info!("Depositing funds");
    // let hex_addr = key.address().to_string();
    faucet::request_ngnt(&addr_hex).await?;

    info!("Creating wallet");
    let provider = Provider::new(Network::Rinkeby);
    let cred = WalletCredentials::from_eth_signer(address, signer, Network::Rinkeby).await?;
    let wallet = Wallet::new(provider, cred).await?;

    let balance = wallet.get_balance(BlockStatus::Committed, TOKEN).await?;
    info!(
        "Deposit successful {} NGNT available",
        utils::big_uint_to_big_dec(balance.clone())
    );

    if wallet.is_signing_key_set().await? == false {
        info!("Unlocking account");
        let unlock = wallet
            .start_change_pubkey()
            .fee_token("GNT")?
            .send()
            .await?;
        debug!("unlock={:?}", unlock);
        unlock.wait_for_commit().await?;
    }

    info!("Obtaining withdrawal fee");
    let amount = utils::big_dec_to_big_uint(BigDecimal::from(args.amount))?;
    let withdraw_fee = wallet
        .provider
        .get_tx_fee(TxFeeTypes::Withdraw, address, TOKEN)
        .await?
        .total_fee;
    info!(
        "Withdrawal transaction fee {:.5}",
        utils::big_uint_to_big_dec(withdraw_fee.clone())
    );

    let total = &amount + &withdraw_fee;
    let withdraw_amount = if balance.cmp(&total) == Ordering::Less {
        warn!("Insufficient funds - withdrawing all remaining balance");
        // I failed to clean the account even if there was 0.1 GNT remaining (Rejection reason:	Not enough balance)
        // see: https://rinkeby.zkscan.io/explorer/accounts/0x92d088f43f688808313c31e5c92ee729e4e0b6bf
        let rounding_error_margin = utils::big_dec_to_big_uint(BigDecimal::from(1.0))?;
        balance - &withdraw_fee - rounding_error_margin
    } else {
        amount
    };

    info!(
        "Withdrawal of {:.5} NGNT started, fee amount {:.5}",
        utils::big_uint_to_big_dec(withdraw_amount.clone()),
        utils::big_uint_to_big_dec(withdraw_fee.clone())
    );

    // let withdraw_amount = withdraw_amount.to_u64().unwrap();
    let withdraw_handle = wallet
        .start_withdraw()
        .token(TOKEN)?
        .amount(withdraw_amount)
        .to(address)
        .send()
        .await?;

    debug!("Withdraw: {:?}", withdraw_handle);

    info!(
        "Waiting for receipt - this takes LOOONG to complete...\nCheck it here: {}",
        format!("https://rinkeby.zkscan.io/explorer/accounts/{}", addr_hex)
    );
    let tx_info = withdraw_handle.wait_for_verify().await?;
    info!("Withdrawal succeeded!\n{:?}", tx_info);

    Ok(())
}
