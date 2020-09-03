use serde::{Deserialize, Serialize};

/// Error message for SGX service bus API.
#[derive(thiserror::Error, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Error {
    #[error("Attestation error: {0}")]
    Attestation(String),
}

pub mod local {
    use super::*;
    pub use graphene::AttestationResponse;
    use ya_service_bus::RpcMessage;

    /// Local SGX bus address.
    pub const BUS_ID: &str = "/local/sgx";

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct VerifyAttestationEvidence {
        pub production: bool,
        pub ias_api_key: String,
        pub ias_nonce: Option<String>,
        pub enclave_quote: Vec<u8>,
    }

    impl RpcMessage for VerifyAttestationEvidence {
        const ID: &'static str = "VerifyAttestationEvidence";
        type Item = AttestationResponse;
        type Error = Error;
    }
}
