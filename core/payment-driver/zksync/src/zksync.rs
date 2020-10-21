// External uses
use async_trait::async_trait;
use tiny_keccak::keccak256;
use web3::types::H160;
use ya_client_model::NodeId;
use zksync::zksync_types::{
    tx::{PackedEthSignature, TxEthSignature},
    Address,
};
use zksync_eth_signer::{error::SignerError, EthereumSigner, RawTransaction};

// Workspace uses

// Local uses
use crate::utils::get_sign_tx;

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
        log::debug!("YagnaEthSigner sign_message({})", hex::encode(message));
        let msg_as_bytes = message_to_signable_bytes(message, true);
        let sign_tx = get_sign_tx(self.eth_address.as_bytes().into());
        let signature = sign_tx(msg_as_bytes).await;
        let signature = convert_to_eth_byte_order(signature);
        let packed_sig = PackedEthSignature::deserialize_packed(&signature).map_err(
            |_| SignerError::SigningFailed("Failed to pack eth signature".to_string())
        )?;
        let tx_eth_sig = TxEthSignature::EthereumSignature(packed_sig);
        Ok(tx_eth_sig)
    }

    async fn sign_transaction(&self, raw_tx: RawTransaction) -> Result<Vec<u8>, SignerError> {
        info!("sign_transaction");
        todo!();
        Ok(vec![])
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
