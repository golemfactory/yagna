// External uses
pub use client::wallet::PackedEthSignature;
use client::wallet::ETH_SIGN_MESSAGE;
use tiny_keccak::keccak256;
use web3::types::H160;

// Workspace uses

// Local uses
use crate::utils::get_sign_tx;

pub async fn get_zksync_seed(pub_address: H160) -> Vec<u8> {
    info!("Creating zksync seed. address={}", pub_address);
    let address = pub_address.as_bytes().into();
    let sign_tx = get_sign_tx(address);
    let seed = sign_tx(message_to_signable_bytes(ETH_SIGN_MESSAGE.as_bytes(), true)).await;
    convert_to_eth_signature(seed)
}

pub async fn eth_sign_transfer(pub_address: H160, message: String) -> Vec<u8> {
    info!(
        "Signing eth transfer. address={}, message={}",
        pub_address, message
    );
    let address: [u8; 20] = *pub_address.as_fixed_bytes();
    let sign_tx = get_sign_tx(address.into());
    let eth_sign_hex = sign_tx(message_to_signable_bytes(message.as_bytes(), true)).await;
    convert_to_eth_signature(eth_sign_hex)
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

fn convert_to_eth_signature(signature: Vec<u8>) -> Vec<u8> {
    let v = &signature[0];
    let r = &signature[1..33];
    let s = &signature[33..65];
    let mut result = Vec::with_capacity(65);
    result.extend_from_slice(&r);
    result.extend_from_slice(&s);
    result.push(if v % 2 == 1 { 0x1c } else { 0x1b });
    result.into()
}
