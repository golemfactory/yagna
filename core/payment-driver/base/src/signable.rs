use anyhow::anyhow;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json;

use crate::utils;

use ya_client_model::payment::Payment;

/// Trait for objects that can be signed ensuring unified way to
/// convert structs to bytes, so signatures can be verified across multiple machines.
pub trait Signable: Serialize + DeserializeOwned + Clone + PartialEq {
    /// Serialize structure to vector of bytes in canonical representation.
    /// This representation should be binary equal on all machines.
    fn canonicalize(&self) -> anyhow::Result<Vec<u8>> {
        let shareable = self.clone().remove_private_info();
        Ok(serde_json_canonicalizer::to_vec(&shareable)?)
    }

    /// Function should remove all information that shouldn't be sent to other Nodes.
    /// Example: `allocation_id` in `Payment` structure is private information on Requestor
    /// side and shouldn't be shared with Provider.
    /// This step is necessary to create canonical version that can be signed and later validated
    /// by other party.
    fn remove_private_info(self) -> Self;

    /// Hash canonical representation of the structure.
    /// In most cases we don't want to sign arrays of arbitrary length, so we use hash
    /// of canonical representation instead.
    fn hash_canonical(&self) -> anyhow::Result<Vec<u8>> {
        Ok(utils::prepare_signature_hash(&self.canonicalize()?))
    }

    /// Verify if `canonical` representation is equivalent to `self`.
    /// Since we always get structure and bytes with its canonical representation,
    /// then verifying signature is not enough. We need to check if `canonical` was
    /// created from structure itself.
    fn verify_canonical(&self, canonical: &[u8]) -> anyhow::Result<()> {
        let from_canonical = serde_json::from_slice::<Self>(canonical)
            .map_err(|e| anyhow!("Failed to deserialize canonical representation: {e}"))?
            .remove_private_info();
        let reference = self.clone().remove_private_info();

        if reference != from_canonical {
            return Err(anyhow!(
                "Canonical representation doesn't match the structure"
            ));
        }
        Ok(())
    }
}

impl Signable for Payment {
    fn remove_private_info(mut self) -> Self {
        // We remove allocation ID from syncs because allocations are not transferred to peers and
        // their IDs would be unknown to the recipient.
        for agreement_payment in &mut self.agreement_payments.iter_mut() {
            agreement_payment.allocation_id = None;
        }

        for activity_payment in &mut self.activity_payments.iter_mut() {
            activity_payment.allocation_id = None;
        }

        self
    }
}
