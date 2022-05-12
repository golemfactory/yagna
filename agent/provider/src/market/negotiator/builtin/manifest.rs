use std::ops::Not;

use ya_agreement_utils::manifest::{
    decode_manifest, Feature, Signature, CAPABILITIES_PROPERTY, DEMAND_MANIFEST_PROPERTY,
    DEMAND_MANIFEST_SIG_PROPERTY,
};
use ya_agreement_utils::policy::{Keystore, Match, Policy, PolicyConfig};
use ya_agreement_utils::{Error, OfferDefinition};

use crate::market::negotiator::*;

#[derive(Default)]
pub struct ManifestSignature {
    enabled: bool,
    trusted_keys: Keystore,
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

        let manifest = match demand.get_property::<String>(DEMAND_MANIFEST_PROPERTY) {
            Err(Error::NoKey(_)) => return Ok(NegotiationResult::Ready { offer }),
            Err(e) => return rejection(format!("invalid manifest type: {:?}", e)),
            Ok(s) => match decode_manifest(s) {
                Ok(manifest) => manifest,
                Err(e) => return rejection(format!("invalid manifest: {:?}", e)),
            },
        };

        if manifest.features().is_empty() {
            return Ok(NegotiationResult::Ready { offer });
        }

        let pub_key = match verify_signature(demand) {
            Ok(pub_key) => pub_key,
            Err(e) => return rejection(format!("invalid manifest signature: {:?}", e)),
        };

        if self.trusted_keys.contains(pub_key.as_slice()) {
            Ok(NegotiationResult::Ready { offer })
        } else {
            rejection("manifest not signed by a trusted authority".to_string())
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
            trusted_keys: config.trusted_keys.unwrap_or_default(),
        }
    }
}

fn rejection(message: String) -> anyhow::Result<NegotiationResult> {
    Ok(NegotiationResult::Reject {
        message,
        is_final: true,
    })
}

fn verify_signature(demand: &ProposalView) -> anyhow::Result<Vec<u8>> {
    let manifest: String = demand.get_property(DEMAND_MANIFEST_PROPERTY)?;
    log::debug!("manifest: {}", manifest);
    let sig_hex: String = demand.get_property(DEMAND_MANIFEST_SIG_PROPERTY)?;
    log::debug!("sig_hex: {}", sig_hex);
    let sig = Signature::Secp256k1Hex(sig_hex);
    Ok(sig.verify_str(manifest)?)
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
