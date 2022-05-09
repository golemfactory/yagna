use std::ops::Not;

use ya_agreement_utils::agreement::parse_constraints;
use ya_agreement_utils::manifest::{
    Signature, CONSTRAINT_CAPABILITIES_REGEX, DEMAND_CAPABILITIES_PROPERTY,
    DEMAND_MANIFEST_PROPERTY, DEMAND_MANIFEST_SIG_PROPERTY,
};
use ya_agreement_utils::policy::{Keystore, Match, Policy, PolicyConfig};
use ya_agreement_utils::OfferDefinition;

use crate::market::negotiator::*;

const CAPABILITY_INET: &str = "inet";

pub struct ManifestSignature {
    enabled: bool,
    trusted_keys: Keystore,
}

impl Default for ManifestSignature {
    fn default() -> Self {
        Self {
            enabled: false,
            trusted_keys: Keystore::default(),
        }
    }
}

impl NegotiatorComponent for ManifestSignature {
    fn negotiate_step(
        &mut self,
        demand: &ProposalView,
        demand_constraints: &String,
        offer: ProposalView,
    ) -> anyhow::Result<NegotiationResult> {
        if self.enabled.not() {
            return Ok(NegotiationResult::Ready { offer });
        }

        if let Some(capabilities) = parse_constraints(
            demand_constraints.as_str(),
            CONSTRAINT_CAPABILITIES_REGEX,
            1,
        ) {
            if capabilities.contains(CAPABILITY_INET).not() {
                return Ok(NegotiationResult::Ready { offer });
            }
        } else {
            return Ok(NegotiationResult::Ready { offer });
        }

        let pub_key = match verify_signature(demand) {
            Ok(pub_key) => pub_key,
            Err(err) => {
                return Ok(NegotiationResult::Reject {
                    message: format!("manifest has an invalid signature: {:?}", err),
                    is_final: true,
                });
            }
        };

        if self.trusted_keys.contains(pub_key.as_slice()) {
            Ok(NegotiationResult::Ready { offer })
        } else {
            Ok(NegotiationResult::Reject {
                message: format!("manifest not signed by a trusted authority"),
                is_final: true,
            })
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
            match properties.get(DEMAND_CAPABILITIES_PROPERTY) {
                Some(Match::Values(vec)) => vec.contains(&CAPABILITY_INET.to_string()).not(),
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

fn verify_signature(demand: &ProposalView) -> anyhow::Result<Vec<u8>> {
    let manifest: String = demand.get_property(DEMAND_MANIFEST_PROPERTY)?;
    log::error!("manifest: {}", manifest);
    let sig_hex: String = demand.get_property(DEMAND_MANIFEST_SIG_PROPERTY)?;
    log::error!("sig_hex: {}", sig_hex);
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
        assert_eq!(policy.enabled, false);

        let policy = build_policy(
            "TEST \
            --policy-disable-component manifest_signature_validation \
            --policy-trust-property property=value1,value2",
        );
        assert_eq!(policy.enabled, false);

        let policy = build_policy(format!(
            "TEST \
            --policy-disable-component manifest_signature_validation \
            --policy-trust-property {}",
            DEMAND_CAPABILITIES_PROPERTY
        ));
        assert!(!policy.enabled);

        let policy = build_policy(format!(
            "TEST \
            --policy-disable-component manifest_signature_validation \
            --policy-trust-property {}={}",
            DEMAND_CAPABILITIES_PROPERTY, CAPABILITY_INET
        ));
        assert!(!policy.enabled);

        let policy = build_policy(format!(
            "TEST \
            --policy-trust-property {}={}",
            DEMAND_CAPABILITIES_PROPERTY, CAPABILITY_INET
        ));
        assert!(!policy.enabled);

        let policy = build_policy(&format!(
            "TEST \
            --policy-trust-property {}",
            DEMAND_CAPABILITIES_PROPERTY
        ));
        assert!(!policy.enabled);

        let policy = build_policy(
            "TEST \
            --policy-trust-property property=value1,value2",
        );
        assert_eq!(policy.enabled, true);

        let policy = build_policy(
            "TEST \
            --policy-trust-property property",
        );
        assert_eq!(policy.enabled, true);

        let policy = build_policy("TEST");
        assert_eq!(policy.enabled, true);
    }
}
