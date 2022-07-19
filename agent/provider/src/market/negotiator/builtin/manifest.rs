use std::ops::Not;

use ya_agreement_utils::{Error, OfferDefinition};
use ya_manifest_utils::policy::{Keystore, Match, Policy, PolicyConfig};
use ya_manifest_utils::{
    decode_data, decode_manifest, Feature, CAPABILITIES_PROPERTY, DEMAND_MANIFEST_CERT_PROPERTY,
    DEMAND_MANIFEST_PROPERTY, DEMAND_MANIFEST_SIG_ALGORITHM_PROPERTY, DEMAND_MANIFEST_SIG_PROPERTY,
};

use crate::market::negotiator::*;

#[derive(Default)]
pub struct ManifestSignature {
    enabled: bool,
    keystore: Keystore,
}

impl NegotiatorComponent for ManifestSignature {
    fn negotiate_step(
        &mut self,
        demand: &ProposalView,
        offer: ProposalView,
    ) -> anyhow::Result<NegotiationResult> {
        if self.enabled.not() {
            return Ok(NegotiationResult::Ready { offer });
        }

        let data = match demand.get_property::<String>(DEMAND_MANIFEST_PROPERTY) {
            Err(Error::NoKey(_)) => return Ok(NegotiationResult::Ready { offer }),
            Err(e) => return rejection(format!("invalid manifest type: {:?}", e)),
            Ok(s) => match decode_data(s) {
                Ok(manifest) => manifest,
                Err(e) => return rejection(format!("invalid manifest encoding: {:?}", e)),
            },
        };

        let manifest = decode_manifest(&data)?;

        if manifest.features().is_empty() {
            return Ok(NegotiationResult::Ready { offer });
        }

        match self.verify_signature(demand, &data) {
            Err(err) => {
                log::debug!("Failed to verify manifest signature: {}", err);
                rejection("failed to verify manifest signature".to_string())
            }
            Ok(()) => Ok(NegotiationResult::Ready { offer }),
        }
    }

    fn fill_template(
        &mut self,
        offer_template: OfferDefinition,
    ) -> anyhow::Result<OfferDefinition> {
        Ok(offer_template)
    }

    fn on_agreement_terminated(
        &mut self,
        _agreement_id: &str,
        _result: &AgreementResult,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn on_agreement_approved(&mut self, _agreement_id: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

impl From<PolicyConfig> for ManifestSignature {
    fn from(config: PolicyConfig) -> Self {
        let policies = config.policy_set();
        let properties = config.trusted_property_map();

        let enabled = if policies
            .contains(&Policy::ManifestSignatureValidation)
            .not()
        {
            false
        } else {
            match properties.get(CAPABILITIES_PROPERTY) {
                Some(Match::Values(vec)) => vec.contains(&Feature::Inet.to_string()).not(),
                Some(Match::All) => false,
                _ => true,
            }
        };

        ManifestSignature {
            enabled,
            keystore: config.trusted_keys.unwrap_or_default(),
        }
    }
}

impl ManifestSignature {
    /// Verifies fields base64 encoding, then validates certificate, then validates signature, then verifies manifest content and returns it
    fn verify_signature(&self, demand: &ProposalView, data: &[u8]) -> anyhow::Result<()> {
        let sig: String = demand.get_property(DEMAND_MANIFEST_SIG_PROPERTY)?;
        log::debug!("sig_hex: {}", sig);
        let sig_alg: String = demand.get_property(DEMAND_MANIFEST_SIG_ALGORITHM_PROPERTY)?;
        log::debug!("sig_alg: {}", sig_alg);
        let cert: String = demand.get_property(DEMAND_MANIFEST_CERT_PROPERTY)?;
        log::debug!("cert: {}", cert);
        self.keystore.verify_signature(cert, sig, sig_alg, data)
    }
}

fn rejection(message: String) -> anyhow::Result<NegotiationResult> {
    Ok(NegotiationResult::Reject {
        message,
        is_final: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use structopt::StructOpt;

    fn build_policy<S: AsRef<str>>(args: S) -> ManifestSignature {
        let arguments = shlex::split(args.as_ref()).expect("failed to parse arguments");
        PolicyConfig::from_iter(arguments).into()
    }

    #[test]
    fn parse_signature_policy() {
        let policy = build_policy(
            "TEST \
            --policy-disable-component all \
            --policy-trust-property property=value1,value2",
        );
        assert!(!policy.enabled);

        let policy = build_policy(
            "TEST \
            --policy-disable-component manifest_signature_validation \
            --policy-trust-property property=value1,value2",
        );
        assert!(!policy.enabled);

        let policy = build_policy(format!(
            "TEST \
            --policy-disable-component manifest_signature_validation \
            --policy-trust-property {}",
            CAPABILITIES_PROPERTY
        ));
        assert!(!policy.enabled);

        let policy = build_policy(format!(
            "TEST \
            --policy-disable-component manifest_signature_validation \
            --policy-trust-property {}={}",
            CAPABILITIES_PROPERTY,
            Feature::Inet.to_string()
        ));
        assert!(!policy.enabled);

        let policy = build_policy(format!(
            "TEST \
            --policy-trust-property {}={}",
            CAPABILITIES_PROPERTY,
            Feature::Inet.to_string()
        ));
        assert!(!policy.enabled);

        let policy = build_policy(&format!(
            "TEST \
            --policy-trust-property {}",
            CAPABILITIES_PROPERTY
        ));
        assert!(!policy.enabled);

        let policy = build_policy(
            "TEST \
            --policy-trust-property property=value1,value2",
        );
        assert!(policy.enabled);

        let policy = build_policy(
            "TEST \
            --policy-trust-property property",
        );
        assert!(policy.enabled);

        let policy = build_policy("TEST");
        assert!(policy.enabled);
    }
}
