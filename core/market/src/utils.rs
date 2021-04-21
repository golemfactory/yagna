mod agreement_lock;
pub mod display;

pub use agreement_lock::AgreementLock;
/*
use ethsign::Signature;
*/
use ya_core_model::NodeId;

pub fn verify_signature(id: NodeId, signature: Vec<u8>, data: Vec<u8>) -> anyhow::Result<bool, anyhow::Error> {
    todo!();
    /*
    if signature.len() != 65 { return Ok(false) }
    let v = signature[0];
    let r: [u8; 32] = signature[1..33].try_into()?;
    let s: [u8; 32] = signature[33..65].try_into()?;
    let sig = Signature {v, r, s};
    let pub_key = match signature.recover(data.as_slice()) {
        Ok(pub_key) => pub_key,
        Err(_) => return Ok(false),
    }
    Ok(pub_key.address() == &id.into_array())

    */
}
