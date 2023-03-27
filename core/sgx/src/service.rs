use anyhow::Result;
use graphene_sgx::IasClient;
use std::env;
use ya_core_model::sgx::Error;
use ya_core_model::sgx::{AttestationResponse, VerifyAttestationEvidence, BUS_ID};
use ya_service_bus::typed as bus;

pub fn bind_gsb() {
    let _ = bus::bind(BUS_ID, verify_attestation_evidence);
}

async fn verify_attestation_evidence(
    msg: VerifyAttestationEvidence,
) -> Result<AttestationResponse, Error> {
    let ias_api_key = env::var("IAS_API_KEY")
        .map_err(|_| Error::Attestation("IAS_API_KEY variable not set".into()))?;
    let ias = IasClient::new(msg.production, &ias_api_key);
    let response = ias
        .verify_attestation_evidence(&msg.enclave_quote, msg.ias_nonce)
        .await
        .map_err(|e| Error::Attestation(format!("{}", e)))?;
    Ok(response)
}
