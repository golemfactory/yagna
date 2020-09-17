#[macro_use]
extern crate log;

use std::convert::TryInto;
use std::str::FromStr;
use std::pin::Pin;
use std::sync::Arc;

use num::BigUint;

use client::wallet::{ETH_SIGN_MESSAGE, Wallet, Address, PackedEthSignature};
use client::rpc_client::RpcClient;

use ethkey::{EthAccount, Password};
use futures3::Future;
use ya_client_model::NodeId;
use ya_core_model::identity;
use tiny_keccak::keccak256;
// use ya_zksync_driver::utils::get_sign_tx;

#[tokio::main]
async fn main() {
    let log_level = std::env::var("RUST_LOG").unwrap_or("info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();
    info!("signed transaction example.");
    debug!("set log_level to {}.", std::env::var("RUST_LOG").unwrap());

    let requestor_account = EthAccount::load_or_generate("requestor.key.json", "").unwrap();
    let sign_tx = get_sign_tx(requestor_account);
    info!("connected sign_tx");

    let input_to = "d0670f5eA3218bB6A95dD7FAcdCfAC3f19ECAd36";
    let input_token = "GNT";
    let input_amount = "6000000000000000000";
    //let input_pk_seed = "6cae8ce3aaf356922b54a0564dbd7075314183e7cfc4fe8478a9bb7b5f7726a31a189146d53997726b9d77f4edf376280cc3609327705b0b175c8423eb6c59261c";
    //let pk_seed_hex = hex::decode(input_pk_seed).unwrap();

    let provider = RpcClient::new("https://rinkeby-api.zksync.io/jsrpc");
    info!("connected to zksync provider");

    let pub_key_str = "c38F303B15A34Ee3d21FC4777533b0CA9DdA766F";
    let pub_key_addr = Address::from_str(pub_key_str).unwrap();
    //let sign_tx = get_sign_tx(NodeId::from_str(pub_key_str).unwrap());
    info!("creating zksync private key");
    info!("sign message={}", ETH_SIGN_MESSAGE);
    let msg_as_bytes = ETH_SIGN_MESSAGE.as_bytes();
    info!("msg_hex={}", hex::encode(&msg_as_bytes));
    let msg_as_bytes = message_to_signed_bytes(msg_as_bytes, true);
//    let msg_as_bytes = msg_as_bytes.to_vec();
    info!("msg_hex={}", hex::encode(&msg_as_bytes));
    info!("creating zksync wallet from message {:?}", hex::encode(&msg_as_bytes));
    let seed_eth_signature = sign_tx(msg_as_bytes).await;
    info!("creating zksync wallet from seed {:?}", hex::encode(&seed_eth_signature));
    let wallet = Wallet::from_seed(&seed_eth_signature, pub_key_addr, provider);
    //info!("wallet: {:?}", wallet);

    let to = Address::from_str(input_to).unwrap();
    let token = input_token;
    let amount = BigUint::from_str(input_amount).unwrap();

    let (transfer, transfer_eth_sign_message) = wallet.prepare_sync_transfer(
        &to,
        token.to_string(),
        amount,
        None
    ).await;
    info!("transfer: {:?}", transfer);
    info!("transfer_eth_sign_message: {:?}", transfer_eth_sign_message);

    //
    //
    let eth_sign_hex = message_to_signed_bytes(transfer_eth_sign_message.as_bytes(), true);
    info!("eth_sig_hex {:?}", hex::encode(&eth_sign_hex));
    let eth_sign_hex = sign_tx(eth_sign_hex).await;
    info!("eth_sig_hex: {:?}", hex::encode(&eth_sign_hex));
    //let eth_sig_hex = hex::decode("79c2b93604ef97e8ab4cce6bd64b67f9a2cbdef02d7a2cc6bb063acb7e07d1cf77c430759180015161fa8010a178901678a0ffa5f871ac8a4dc8d646421a3f0e1b").expect("failed to decode hex");
    let eth_signature = PackedEthSignature::deserialize_packed(&eth_sign_hex).unwrap();

    let tx_hash = wallet.sync_transfer(transfer, eth_signature).await;

    info!("tx_hash= {}.", hex::encode(tx_hash));
}

fn message_to_signed_bytes(msg: &[u8], include_prefix: bool) -> Vec<u8> {
    let bytes = if include_prefix {
        let prefix = format!("\x19Ethereum Signed Message:\n{}", msg.len());
        let mut b = Vec::with_capacity(prefix.len() + msg.len());
        b.extend_from_slice(prefix.as_bytes());
        b.extend_from_slice(msg);
        b
    }
    else {
        msg.into()
    };
    tiny_keccak::keccak256(&bytes).into()
}

fn get_sign_tx(
    account: Box<EthAccount>,
) -> impl Fn(Vec<u8>) -> Pin<Box<dyn Future<Output = Vec<u8>>>> {
    let account: Arc<EthAccount> = account.into();
    move |msg| {
        let account = account.clone();
        let fut = async move {
            let msg: [u8; 32] = msg.as_slice().try_into().unwrap();
            let signature = account.sign(&msg).unwrap();
            info!("Signature: {:?}", signature);
            let mut v = Vec::with_capacity(65);
            //v.push(signature.v);
            v.extend_from_slice(&signature.r);
            v.extend_from_slice(&signature.s);
            v.push(if signature.v % 2 == 1 { 0x1c } else { 0x1b });
            v
        };
        Box::pin(fut)
    }
}
