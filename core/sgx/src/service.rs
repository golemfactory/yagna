use anyhow::Result;
use graphene::ias::IasClient;
use ya_core_model::sgx::{AttestationResponse, Error, VerifyAttestationEvidence, BUS_ID};
use ya_service_bus::typed as bus;

pub fn bind_gsb() {
    let _ = bus::bind(&BUS_ID, verify_attestation_evidence);
}

async fn verify_attestation_evidence(
    msg: VerifyAttestationEvidence,
) -> Result<AttestationResponse, Error> {
    let ias = IasClient::new(msg.production);
    let response = ias
        .verify_attestation_evidence(&msg.enclave_quote, &msg.ias_api_key, msg.ias_nonce)
        .await
        .map_err(|e| Error::Attestation(format!("{}", e)))?;
    Ok(response)
}
