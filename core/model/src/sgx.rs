pub use graphene_sgx::AttestationResponse;
use serde::{Deserialize, Serialize};
use ya_service_bus::RpcMessage;

/// Public SGX bus address.
pub const BUS_ID: &str = "/public/sgx";

/// Error message for SGX service bus API.
#[derive(thiserror::Error, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Error {
    #[error("Attestation error: {0}")]
    Attestation(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyAttestationEvidence {
    pub production: bool,
    pub ias_nonce: Option<String>,
    pub enclave_quote: Vec<u8>,
}

impl RpcMessage for VerifyAttestationEvidence {
    const ID: &'static str = "VerifyAttestationEvidence";
    type Item = AttestationResponse;
    type Error = Error;
}
