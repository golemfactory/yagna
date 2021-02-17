#[macro_use]
extern crate log;

use std::convert::TryInto;
use std::str::FromStr;

use async_trait::async_trait;
use num_bigint::BigUint;

use zksync::zksync_types::{
    tx::{PackedEthSignature, TxEthSignature},
    Address,
};
use zksync::{types::BlockStatus, Network};
use zksync::{RpcProvider, Wallet, WalletCredentials};
use zksync_eth_signer::{error::SignerError, EthereumSigner, RawTransaction};

use ethkey::EthAccount;
use tiny_keccak::keccak256;

struct YagnaEthSigner {
    eth_address: Address,
}

impl YagnaEthSigner {
    pub fn new(eth_address: Address) -> Self {
        Self { eth_address }
    }
}

impl Clone for YagnaEthSigner {
    fn clone(&self) -> Self {
        todo!()
    }
}

#[async_trait]
impl EthereumSigner for YagnaEthSigner {
    async fn get_address(&self) -> Result<Address, SignerError> {
        Ok(self.eth_address)
    }

    async fn sign_message(&self, message: &[u8]) -> Result<TxEthSignature, SignerError> {
        let msg_as_bytes = message_to_signable_bytes(message, true);

        let requestor_account = EthAccount::load_or_generate("requestor.key.json", "").unwrap();
        info!("connected sign_tx");

        let signature = sign_tx(requestor_account, msg_as_bytes);
        info!("got signature");
        let signature = convert_to_eth_byte_order(signature);
        info!("put signature in order");
        // TODO: map error
        let packed_sig = PackedEthSignature::deserialize_packed(&signature).unwrap();
        info!("packed signature");
        let tx_eth_sig = TxEthSignature::EthereumSignature(packed_sig);
        info!("final result");

        Ok(tx_eth_sig)
    }

    async fn sign_transaction(&self, _raw_tx: RawTransaction) -> Result<Vec<u8>, SignerError> {
        info!("sign_transaction");
        Ok(vec![])
    }
}

#[tokio::main]
async fn main() {
    let log_level = std::env::var("RUST_LOG").unwrap_or("info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();
    info!("signed transaction example.");
    debug!("set log_level to {}.", std::env::var("RUST_LOG").unwrap());

    // let requestor_account = EthAccount::load_or_generate("requestor.key.json", "").unwrap();
    // let sign_tx = get_sign_tx(requestor_account);
    info!("connected sign_tx");

    let input_to = "d0670f5eA3218bB6A95dD7FAcdCfAC3f19ECAd36";
    let input_token = "GNT";
    let input_amount = "1000000000000000000";
    //let input_pk_seed = "6cae8ce3aaf356922b54a0564dbd7075314183e7cfc4fe8478a9bb7b5f7726a31a189146d53997726b9d77f4edf376280cc3609327705b0b175c8423eb6c59261c";
    //let pk_seed_hex = hex::decode(input_pk_seed).unwrap();

    let pub_key_str = "917605f5e18817ca72cab34dfbb34fe197fc2616";
    let pub_key_addr = Address::from_str(pub_key_str).unwrap();
    let provider = RpcProvider::new(Network::Rinkeby);
    let ext_eth_signer = YagnaEthSigner::new(pub_key_addr);
    info!("connected to zksync provider");

    let creds = WalletCredentials::from_eth_signer(pub_key_addr, ext_eth_signer, Network::Rinkeby)
        .await
        .unwrap();
    info!("created credentials");

    let wallet = Wallet::new(provider, creds).await.unwrap();
    info!("created wallet");

    let balance = wallet
        .get_balance(BlockStatus::Committed, "GNT")
        .await
        .unwrap();
    info!("balance={}", balance);

    if wallet.is_signing_key_set().await.unwrap() == false {
        let unlock = wallet
            .start_change_pubkey()
            .fee_token("GNT")
            .unwrap()
            .send()
            .await
            .unwrap();
        info!("unlock={:?}", unlock);
    }

    let transfer = wallet
        .start_transfer()
        .str_to(input_to)
        .unwrap()
        .token(input_token)
        .unwrap()
        .amount(BigUint::from_str(input_amount).unwrap())
        .send()
        .await
        .unwrap();

    info!("tx_hash= {}", hex::encode(transfer.hash()));
}

fn message_to_signable_bytes(msg: &[u8], include_prefix: bool) -> Vec<u8> {
    let bytes = if include_prefix {
        let prefix = format!("\x19Ethereum Signed Message:\n{}", msg.len());
        let mut b = Vec::with_capacity(prefix.len() + msg.len());
        b.extend_from_slice(prefix.as_bytes());
        b.extend_from_slice(msg);
        b
    } else {
        msg.into()
    };
    keccak256(&bytes).into()
}

fn convert_to_eth_byte_order(signature: Vec<u8>) -> Vec<u8> {
    let v = &signature[0];
    let r = &signature[1..33];
    let s = &signature[33..65];
    let mut result = Vec::with_capacity(65);
    result.extend_from_slice(&r);
    result.extend_from_slice(&s);
    result.push(if v % 2 == 1 { 0x1c } else { 0x1b });
    result
}

fn sign_tx(account: Box<EthAccount>, msg: Vec<u8>) -> Vec<u8> {
    let msg: [u8; 32] = msg.as_slice().try_into().unwrap();
    let signature = account.sign(&msg).unwrap();
    info!("Signature: {:?}", signature);
    let mut v = Vec::with_capacity(65);
    v.push(signature.v);
    v.extend_from_slice(&signature.r);
    v.extend_from_slice(&signature.s);
    v
}
