use std::collections::HashSet;
use std::ops::Not;

use url::Url;
use ya_agreement_utils::{Error, OfferDefinition};
use ya_manifest_utils::matching::domain::{DomainWhitelistState, SharedDomainMatchers};
use ya_manifest_utils::matching::Matcher;
use ya_manifest_utils::policy::{CertPermissions, Keystore, Match, Policy, PolicyConfig};
use ya_manifest_utils::rules::RuleStore;
use ya_manifest_utils::{
    decode_manifest, AppManifest, Feature, CAPABILITIES_PROPERTY,
    DEMAND_MANIFEST_CERT_PERMISSIONS_PROPERTY, DEMAND_MANIFEST_CERT_PROPERTY,
    DEMAND_MANIFEST_PROPERTY, DEMAND_MANIFEST_SIG_ALGORITHM_PROPERTY, DEMAND_MANIFEST_SIG_PROPERTY,
};

use crate::market::negotiator::*;

//TODO Rafał Rename
#[derive(Clone, Debug, Default)]
pub struct PolicyStruct {
    pub trusted_keys: Keystore,
    pub domain_patterns: DomainWhitelistState,
    pub rules_config: RuleStore,
}

pub struct ManifestSignature {
    enabled: bool,
    keystore: Keystore,
    rulestore: RuleStore,
    whitelist_matcher: SharedDomainMatchers,
}

impl NegotiatorComponent for ManifestSignature {
    fn negotiate_step(
        &mut self,
        demand: &ProposalView,
        offer: ProposalView,
    ) -> anyhow::Result<NegotiationResult> {
        if self.enabled.not() || self.rulestore.always_accept_outbound() {
            log::trace!("Manifest signature verification disabled.");
            return acceptance(offer);
        }

        let demand = match demand.get_property::<String>(DEMAND_MANIFEST_PROPERTY) {
            Ok(manifest_encoded) => match decode_manifest(&manifest_encoded) {
                Ok(manifest) => DemandWithManifest {
                    demand,
                    manifest_encoded,
                    manifest,
                },
                Err(e) => return rejection(format!("invalid manifest: {:?}", e)),
            },
            Err(Error::NoKey(_)) => return acceptance(offer),
            Err(e) => return rejection(format!("invalid manifest type: {:?}", e)),
        };

        if demand.has_signature() {
            match demand.verify_signature(&self.keystore) {
                Ok(()) => match demand.verify_permissions(&self.keystore) {
                    Ok(_) => acceptance(offer),
                    Err(e) => rejection(format!("certificate permissions verification: {e}")),
                },
                Err(e) => rejection(format!("failed to verify manifest signature: {e}")),
            }
        } else if demand.requires_signature(&self.whitelist_matcher) {
            rejection("manifest requires signature but it has none".to_string())
        } else {
            log::trace!("No signature required. No signature provided.");
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
    pub fn new(config: &PolicyConfig, x: PolicyStruct) -> Self {
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

        let whitelist_matcher = x.domain_patterns.matchers.clone();
        //TODO Nones should be errors or config should not wrap stores inside Option
        let keystore = x.trusted_keys;
        let rulestore = x.rules_config;
        ManifestSignature {
            enabled,
            keystore,
            rulestore,
            whitelist_matcher,
        }
    }
}

struct DemandWithManifest<'demand> {
    demand: &'demand ProposalView,
    manifest_encoded: String,
    manifest: AppManifest,
}

impl<'demand> DemandWithManifest<'demand> {
    fn has_signature(&self) -> bool {
        self.demand
            .get_property::<String>(DEMAND_MANIFEST_SIG_PROPERTY)
            .is_ok()
    }

    fn requires_signature(&self, whitelist_matcher: &SharedDomainMatchers) -> bool {
        let features = self.manifest.features();
        if features.is_empty() {
            log::debug!("No features in demand. Signature not required.");
            return false;
        // Inet is the only feature
        } else if features.contains(&Feature::Inet) && features.len() == 1 {
            if let Some(urls) = self
                .manifest
                .comp_manifest
                .as_ref()
                .and_then(|comp| comp.net.as_ref())
                .and_then(|net| net.inet.as_ref())
                .and_then(|inet| inet.out.as_ref())
                .and_then(|out| out.urls.as_ref())
            {
                let matcher = whitelist_matcher.read().unwrap();
                let non_whitelisted_urls: Vec<&str> = urls
                    .iter()
                    .flat_map(Url::host_str)
                    .filter(|domain| matcher.matches(domain).not())
                    .collect();
                if non_whitelisted_urls.is_empty() {
                    log::debug!("Demand does not require signature. Every URL on whitelist");
                    return false;
                }
                log::debug!(
                    "Demand requires signature. Non whitelisted URLs: {:?}",
                    non_whitelisted_urls
                );
                return true;
            }
        }
        log::debug!("Demand requires signature.");
        true
    }

    fn verify_signature(&self, keystore: &Keystore) -> anyhow::Result<()> {
        let sig = self
            .demand
            .get_property::<String>(DEMAND_MANIFEST_SIG_PROPERTY)?;
        log::trace!("sig_hex: {}", sig);
        let sig_alg: String = self
            .demand
            .get_property(DEMAND_MANIFEST_SIG_ALGORITHM_PROPERTY)?;
        log::trace!("sig_alg: {}", sig_alg);
        let cert: String = self.demand.get_property(DEMAND_MANIFEST_CERT_PROPERTY)?;
        log::trace!("cert: {}", cert);
        log::trace!("manifest: {}", &self.manifest_encoded);
        keystore.verify_signature(cert, sig, sig_alg, &self.manifest_encoded)
    }

    fn verify_permissions(&self, keystore: &Keystore) -> anyhow::Result<()> {
        let mut required = required_permissions(&self.manifest.features());
        let cert: String = self.demand.get_property(DEMAND_MANIFEST_CERT_PROPERTY)?;

        if self
            .demand
            .get_property::<String>(DEMAND_MANIFEST_CERT_PERMISSIONS_PROPERTY)
            .is_ok()
        {
            // Verification of certificate permissions defined in demand is NYI.
            // To make Provider accept Demand containig Certificates Permissions it is required to
            // add Certificate with "unverified-permissions-chain" permission into the keystore.
            required.push(CertPermissions::UnverifiedPermissionsChain);
        }

        keystore.verify_permissions(&cert, required)
    }
}

fn required_permissions(features: &HashSet<Feature>) -> Vec<CertPermissions> {
    features
        .iter()
        .filter_map(|feature| match feature {
            Feature::Inet => Some(CertPermissions::OutboundManifest),
            _ => None,
        })
        .collect()
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

    fn build_policy<S: AsRef<str>>(args: S) -> ManifestSignature {
        let arguments = shlex::split(args.as_ref()).expect("failed to parse arguments");
        ManifestSignature::new(
            &PolicyConfig::from_iter(arguments).into(),
            PolicyStruct {
                trusted_keys: Default::default(),
                domain_patterns: Default::default(),
                rules_config: Default::default(),
            },
        )
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
            Feature::Inet
        ));
        assert!(!policy.enabled);

        let policy = build_policy(format!(
            "TEST \
            --policy-trust-property {}={}",
            CAPABILITIES_PROPERTY,
            Feature::Inet
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
