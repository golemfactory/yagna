// External uses
use async_trait::async_trait;
use bigdecimal::{BigDecimal, Zero};
use num::BigUint;
use tiny_keccak::keccak256;
use zksync::{Provider, Network};
use zksync::zksync_types::{tx::{PackedEthSignature, TxEthSignature}, Address, H160};
use zksync_eth_signer::{error::SignerError, EthereumSigner, RawTransaction};

// Workspace uses

// Local uses
use crate::utils::{sign_tx, big_uint_to_big_dec};
use crate::ZKSYNC_TOKEN_NAME;
use ya_core_model::driver::GenericError;

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
    // TODO: Make chainid from a config like GNT driver
    let provider = Provider::new(Network::Rinkeby);
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
