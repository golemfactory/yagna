use std::ops::Not;

use ya_agreement_utils::{Error, OfferDefinition};
use ya_manifest_utils::policy::{Match, Policy, PolicyConfig};
use ya_manifest_utils::{
    decode_manifest, Feature, CAPABILITIES_PROPERTY, DEMAND_MANIFEST_CERT_PROPERTY,
    DEMAND_MANIFEST_NODE_DESCRIPTOR_PROPERTY, DEMAND_MANIFEST_PROPERTY,
    DEMAND_MANIFEST_SIG_ALGORITHM_PROPERTY, DEMAND_MANIFEST_SIG_PROPERTY,
};

use crate::market::negotiator::*;
use crate::provider_agent::AgentNegotiatorsConfig;
use crate::rules::{ManifestSignatureProps, RulesManager};

pub struct ManifestSignature {
    enabled: bool,
    rules_manager: RulesManager,
}

impl NegotiatorComponent for ManifestSignature {
    fn negotiate_step(
        &mut self,
        demand: &ProposalView,
        offer: ProposalView,
    ) -> anyhow::Result<NegotiationResult> {
        if self.enabled.not() {
            log::trace!("Manifest verification disabled.");
            return acceptance(offer);
        }

        let (manifest, manifest_encoded) =
            match demand.get_property::<String>(DEMAND_MANIFEST_PROPERTY) {
                Ok(manifest_encoded) => match decode_manifest(&manifest_encoded) {
                    Ok(manifest) => (manifest, manifest_encoded),
                    Err(e) => return rejection(format!("invalid manifest: {:?}", e)),
                },
                Err(Error::NoKey(_)) => return acceptance(offer),
                Err(e) => return rejection(format!("invalid manifest type: {:?}", e)),
            };

        let manifest_sig = {
            if demand
                .get_property::<String>(DEMAND_MANIFEST_SIG_PROPERTY)
                .is_ok()
            {
                let sig = demand.get_property::<String>(DEMAND_MANIFEST_SIG_PROPERTY)?;
                log::trace!("sig_hex: {sig}");
                let sig_alg: String =
                    demand.get_property(DEMAND_MANIFEST_SIG_ALGORITHM_PROPERTY)?;
                log::trace!("sig_alg: {sig_alg}");
                let cert: String = demand.get_property(DEMAND_MANIFEST_CERT_PROPERTY)?;
                log::trace!("cert: {cert}");
                log::trace!("encoded_manifest: {manifest_encoded}");
                Some(ManifestSignatureProps {
                    sig,
                    sig_alg,
                    cert,
                    manifest_encoded,
                })
            } else {
                None
            }
        };

        let node_descriptor = demand
            .get_property::<serde_json::Value>(DEMAND_MANIFEST_NODE_DESCRIPTOR_PROPERTY)
            .ok();

        if manifest.is_outbound_requested() {
            match self.rules_manager.check_outbound_rules(
                manifest,
                demand.issuer,
                manifest_sig,
                node_descriptor,
            ) {
                crate::rules::CheckRulesResult::Accept => acceptance(offer),
                crate::rules::CheckRulesResult::Reject(msg) => rejection(msg),
            }
        } else {
            log::trace!("Outbound is not requested.");
            acceptance(offer)
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

impl ManifestSignature {
    pub fn new(config: &PolicyConfig, agent_negotiators_cfg: AgentNegotiatorsConfig) -> Self {
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
            rules_manager: agent_negotiators_cfg.rules_manager,
        }
    }
}

fn rejection(message: String) -> anyhow::Result<NegotiationResult> {
    Ok(NegotiationResult::Reject {
        message,
        is_final: true,
    })
}

fn acceptance(offer: ProposalView) -> anyhow::Result<NegotiationResult> {
    Ok(NegotiationResult::Ready { offer })
}

#[cfg(test)]
mod tests {
    use super::*;
    use structopt::StructOpt;
    use tempdir::TempDir;

    fn build_policy<S: AsRef<str>>(args: S) -> (ManifestSignature, TempDir) {
        let tempdir = TempDir::new("test_dir").unwrap();
        let rules_file = tempdir.path().join("rules.json");
        let whitelist_file = tempdir.path().join("whitelist.json");
        let cert_dir = tempdir.path().join("cert_dir");

        let arguments = shlex::split(args.as_ref()).expect("failed to parse arguments");

        (
            ManifestSignature::new(
                &PolicyConfig::from_iter(arguments),
                AgentNegotiatorsConfig {
                    rules_manager: RulesManager::load_or_create(
                        &rules_file,
                        &whitelist_file,
                        &cert_dir,
                    )
                    .unwrap(),
                },
            ),
            tempdir,
        )
    }

    #[test]
    fn parse_signature_policy() {
        let (policy, _tmpdir) = build_policy(
            "TEST \
            --policy-disable-component all \
            --policy-trust-property property=value1,value2",
        );
        assert!(!policy.enabled);

        let (policy, _tmpdir) = build_policy(
            "TEST \
            --policy-disable-component manifest_signature_validation \
            --policy-trust-property property=value1,value2",
        );
        assert!(!policy.enabled);

        let (policy, _tmpdir) = build_policy(format!(
            "TEST \
            --policy-disable-component manifest_signature_validation \
            --policy-trust-property {}",
            CAPABILITIES_PROPERTY
        ));
        assert!(!policy.enabled);

        let (policy, _tmpdir) = build_policy(format!(
            "TEST \
            --policy-disable-component manifest_signature_validation \
            --policy-trust-property {}={}",
            CAPABILITIES_PROPERTY,
            Feature::Inet
        ));
        assert!(!policy.enabled);

        let (policy, _tmpdir) = build_policy(format!(
            "TEST \
            --policy-trust-property {}={}",
            CAPABILITIES_PROPERTY,
            Feature::Inet
        ));
        assert!(!policy.enabled);

        let (policy, _tmpdir) = build_policy(format!(
            "TEST \
            --policy-trust-property {}",
            CAPABILITIES_PROPERTY
        ));
        assert!(!policy.enabled);

        let (policy, _tmpdir) = build_policy(
            "TEST \
            --policy-trust-property property=value1,value2",
        );
        assert!(policy.enabled);

        let (policy, _tmpdir) = build_policy(
            "TEST \
            --policy-trust-property property",
        );
        assert!(policy.enabled);

        let (policy, _tmpdir) = build_policy("TEST");
        assert!(policy.enabled);
    }
}
