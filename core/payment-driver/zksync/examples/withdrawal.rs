#[macro_use]
extern crate log;

use bigdecimal::{BigDecimal, ToPrimitive};
use hex::ToHex;
use num::BigUint;
use ya_zksync_driver::zksync::{faucet, utils};
use zksync::zksync_types::H256;
use zksync::{types::BlockStatus, Network, Provider, Wallet, WalletCredentials};
use zksync_eth_signer::{EthereumSigner, PrivateKeySigner};

const TOKEN: &str = "GNT";

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let log_level = std::env::var("RUST_LOG").unwrap_or("info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();

    let private_key = H256::random();
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
        utils::big_uint_to_big_dec(balance)
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

    info!("Withdrawal started");
    let gnt_10 = utils::big_dec_to_big_uint(BigDecimal::from(10))?;
    let withdraw_amount: u64 = BigUint::to_u64(&gnt_10).unwrap();
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
