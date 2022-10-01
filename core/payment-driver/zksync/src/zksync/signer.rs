/*
    Handle zksync signing.

    - EthereumSigner trait
    - ya_service_bus connection to sign
    - Helpers to convert byte formats/orders
*/

// External uses
use async_trait::async_trait;
use futures::{Future, FutureExt};
use rlp::RlpStream;
use std::pin::Pin;
use tiny_keccak::keccak256;
use tokio::task;
use zksync::zksync_types::{
    tx::{PackedEthSignature, TxEthSignature},
    Address,
};
use zksync_eth_signer::{error::SignerError, EthereumSigner, RawTransaction};

// Workspace uses
use ya_client_model::NodeId;
use ya_payment_driver::bus;

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
        Self::new(self.eth_address)
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

    async fn sign_transaction(&self, raw_tx: RawTransaction) -> Result<Vec<u8>, SignerError> {
        log::debug!("YagnaEthSigner sign_transaction");

        let node_id = self.eth_address.as_bytes().into();
        let payload: Vec<u8> = raw_tx.hash().into();
        let chain_id = raw_tx.chain_id as u64;

        let signature = sign_tx(node_id, payload.clone()).await?;

        Ok(encode_signed_tx(&raw_tx, signature, chain_id))
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
    // Yagna byte order    (v, r s)
    // Ethereum byte order (r, s, (v % 2 + 28))
    let v = &signature[0];
    let r = &signature[1..33];
    let s = &signature[33..65];
    let mut result = Vec::with_capacity(65);
    result.extend_from_slice(&r);
    result.extend_from_slice(&s);
    result.push(if v % 2 == 1 { 0x1c } else { 0x1b });
    result.into()
}

fn sign_tx(
    node_id: NodeId,
    payload: Vec<u8>,
) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, SignerError>> + Send>> {
    // The zksync EthereumAccount requires "Send", while the bus can not use "Send".
    let fut = task::spawn_local(async move {
        let signature = bus::sign(node_id, payload)
            .await
            .map_err(|e| SignerError::SigningFailed(format!("{:?}", e)))?;
        Ok(signature)
    });
    let fut = fut.map(|res| match res {
        Ok(res) => res,
        Err(e) => Err(SignerError::SigningFailed(e.to_string())),
    });
    Box::pin(fut)
}

fn encode_signed_tx(raw_tx: &RawTransaction, signature: Vec<u8>, chain_id: u64) -> Vec<u8> {
    let (sig_v, sig_r, sig_s) = prepare_signature(signature, chain_id);

    let mut tx = RlpStream::new();

    tx.begin_unbounded_list();

    tx_encode(&raw_tx, &mut tx);
    tx.append(&sig_v);
    tx.append(&sig_r);
    tx.append(&sig_s);

    tx.finalize_unbounded_list();

    tx.out().to_vec()
}

fn prepare_signature(mut signature: Vec<u8>, chain_id: u64) -> (u64, Vec<u8>, Vec<u8>) {
    // TODO ugly solution
    assert_eq!(signature.len(), 65);

    let sig_v = signature[0];
    let sig_v = sig_v as u64 + chain_id * 2 + 35;

    let mut sig_r = signature.split_off(1);
    let mut sig_s = sig_r.split_off(32);

    prepare_signature_part(&mut sig_r);
    prepare_signature_part(&mut sig_s);

    (sig_v, sig_r, sig_s)
}

fn prepare_signature_part(part: &mut Vec<u8>) {
    assert_eq!(part.len(), 32);
    while part[0] == 0 {
        part.remove(0);
    }
}

fn tx_encode(tx: &RawTransaction, s: &mut RlpStream) {
    s.append(&tx.nonce);
    s.append(&tx.gas_price);
    s.append(&tx.gas);
    if let Some(ref t) = tx.to {
        s.append(t);
    } else {
        s.append(&vec![]);
    }
    s.append(&tx.value);
    s.append(&tx.data);
}
