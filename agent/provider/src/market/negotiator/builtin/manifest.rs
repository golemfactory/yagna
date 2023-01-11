use std::collections::HashSet;
use std::ops::Not;

use url::Url;
use ya_agreement_utils::{Error, OfferDefinition};
use ya_manifest_utils::matching::domain::SharedDomainMatchers;
use ya_manifest_utils::matching::Matcher;
use ya_manifest_utils::policy::{CertPermissions, Keystore, Match, Policy, PolicyConfig};
use ya_manifest_utils::{
    decode_manifest, AppManifest, Feature, CAPABILITIES_PROPERTY,
    DEMAND_MANIFEST_CERT_PERMISSIONS_PROPERTY, DEMAND_MANIFEST_CERT_PROPERTY,
    DEMAND_MANIFEST_PROPERTY, DEMAND_MANIFEST_SIG_ALGORITHM_PROPERTY, DEMAND_MANIFEST_SIG_PROPERTY,
};

use crate::market::negotiator::*;
use crate::provider_agent::AgentNegotiatorsConfig;
use crate::rules::RuleStore;

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
        if self.enabled.not() {
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

        if demand.manifest.is_outbound_requested() {
            match self.rulestore.check_outbound_rules(
                demand,
                &self.keystore,
                &self.whitelist_matcher,
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
            keystore: agent_negotiators_cfg.trusted_keys,
            rulestore: agent_negotiators_cfg.rules_config,
            whitelist_matcher: agent_negotiators_cfg.domain_patterns.matchers,
        }
    }
}

//TODO Rafał move it / not pass to rulestore
pub struct DemandWithManifest<'demand> {
    demand: &'demand ProposalView,
    manifest_encoded: String,
    manifest: AppManifest,
}

impl<'demand> DemandWithManifest<'demand> {
    pub fn has_signature(&self) -> bool {
        self.demand
            .get_property::<String>(DEMAND_MANIFEST_SIG_PROPERTY)
            .is_ok()
    }

    pub fn whitelist_matching(&self, whitelist_matcher: &SharedDomainMatchers) -> bool {
        //TODO Rafał Refactor + why there was Inet if?
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
                log::debug!("Every URL on whitelist");
                return true;
            }
            log::debug!("Whitelis. Non whitelisted URLs: {:?}", non_whitelisted_urls);
            return false;
        }
        //TODO Rafał is it right?
        log::debug!("No url's to check");
        true
    }

    pub fn verify_signature(&self, keystore: &Keystore) -> anyhow::Result<()> {
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

    pub fn verify_permissions(&self, keystore: &Keystore) -> anyhow::Result<()> {
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
            &PolicyConfig::from_iter(arguments),
            AgentNegotiatorsConfig {
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
