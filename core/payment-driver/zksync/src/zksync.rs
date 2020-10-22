// External uses
use async_trait::async_trait;
use bigdecimal::{BigDecimal, Zero};
use lazy_static::lazy_static;
use num::BigUint;
use std::env;
use std::str::FromStr;
use tiny_keccak::keccak256;
use zksync::zksync_types::{
    tx::{PackedEthSignature, TxEthSignature},
    Address, H160,
};
use zksync::{Network, Provider, Wallet, WalletCredentials};
use zksync_eth_signer::{error::SignerError, EthereumSigner, RawTransaction};

// Workspace uses
use ya_core_model::driver::GenericError;

// Local uses
use crate::utils::{big_uint_to_big_dec, sign_tx};
use crate::ZKSYNC_TOKEN_NAME;

lazy_static! {
    pub static ref NETWORK: Network = {
        let chain_id = env::var("CHAIN_ID")
            .unwrap_or("rinkeby".to_string())
            .to_lowercase();
        match chain_id.as_str() {
            "rinkeby" => Network::Rinkeby,
            "mainnet" => Network::Mainnet,
            _ => panic!(format!("Invalid chain id: {}", chain_id)),
        }
    };
    static ref PROVIDER: Provider = Provider::new(*NETWORK);
}

pub fn get_provider() -> Provider {
    (*PROVIDER).clone()
}

pub async fn get_wallet(addr: &str) -> Result<Wallet<YagnaEthSigner>, GenericError> {
    let addr = Address::from_str(&addr[2..]).map_err(GenericError::new)?;
    let provider = get_provider();
    let signer = YagnaEthSigner::new(addr);
    let credentials = WalletCredentials::from_eth_signer(addr, signer, *NETWORK)
        .await
        .map_err(GenericError::new)?;
    let wallet = Wallet::new(provider, credentials)
        .await
        .map_err(GenericError::new)?;
    Ok(wallet)
}

pub async fn unlock_wallet<S: EthereumSigner + Clone>(
    wallet: Wallet<S>,
) -> Result<(), GenericError> {
    if !wallet
        .is_signing_key_set()
        .await
        .map_err(GenericError::new)?
    {
        log::info!("Unlocking wallet... address = {}", wallet.signer.address);
        let unlock = wallet
            .start_change_pubkey()
            .fee_token(ZKSYNC_TOKEN_NAME)
            .map_err(GenericError::new)?
            .send()
            .await
            .map_err(GenericError::new)?;
        info!("Unlock tx: {:?}", unlock);
        let tx_info = unlock.wait_for_commit().await.map_err(GenericError::new)?;
        log::info!("Wallet unlocked. tx_info = {:?}", tx_info);
    }
    Ok(())
}

pub struct YagnaEthSigner {
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
        log::debug!("YagnaEthSigner sign_message({})", hex::encode(message));
        let node_id = self.eth_address.as_bytes().into();
        let msg_as_bytes = message_to_signable_bytes(message, true);
        let signature = sign_tx(node_id, msg_as_bytes).await?;
        let signature = convert_to_eth_byte_order(signature);
        let packed_sig = PackedEthSignature::deserialize_packed(&signature)
            .map_err(|_| SignerError::SigningFailed("Failed to pack eth signature".to_string()))?;
        let tx_eth_sig = TxEthSignature::EthereumSignature(packed_sig);
        Ok(tx_eth_sig)
    }

    async fn sign_transaction(&self, _raw_tx: RawTransaction) -> Result<Vec<u8>, SignerError> {
        log::debug!("YagnaEthSigner sign_transaction");
        todo!();
    }
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
    result.into()
}

pub async fn account_balance(addr: H160) -> Result<BigDecimal, GenericError> {
    let provider = get_provider();
    let acc_info = provider
        .account_info(addr)
        .await
        .map_err(GenericError::new)?;
    let balance_com = acc_info
        .committed
        .balances
        .get(ZKSYNC_TOKEN_NAME)
        .map(|x| x.0.clone())
        .unwrap_or(BigUint::zero());
    Ok(big_uint_to_big_dec(balance_com))
}
