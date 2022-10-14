use actix::Message;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_yaml;
use std::ops::Not;
use std::path::PathBuf;
use structopt::StructOpt;

use url::Url;
use ya_agreement_utils::{Error, ProposalView};
use ya_manifest_utils::matching::domain::{
    DomainWhitelistState, DomainsMatcher, SharedDomainMatchers,
};
use ya_manifest_utils::matching::Matcher;
use ya_manifest_utils::policy::{Keystore, Match, Policy, PolicyConfig};
use ya_manifest_utils::{
    decode_manifest, AppManifest, Feature, CAPABILITIES_PROPERTY, DEMAND_MANIFEST_CERT_PROPERTY,
    DEMAND_MANIFEST_PROPERTY, DEMAND_MANIFEST_SIG_ALGORITHM_PROPERTY, DEMAND_MANIFEST_SIG_PROPERTY,
};
use ya_negotiators::component::{RejectReason, Score};
use ya_negotiators::factory::{LoadMode, NegotiatorConfig};
use ya_negotiators::{NegotiationResult, NegotiatorComponent};

use crate::market::negotiator::*;

#[derive(Default)]
pub struct ManifestSignature {
    enabled: bool,
    keystore: Keystore,
    whitelist_matcher: DomainsMatcher,
}

impl NegotiatorComponent for ManifestSignature {
    fn negotiate_step(
        &mut self,
        their: &ProposalView,
        ours: ProposalView,
        score: Score,
    ) -> anyhow::Result<NegotiationResult> {
        if self.enabled.not() {
            return Ok(NegotiationResult::Ready {
                proposal: ours,
                score,
            });
            log::trace!("Manifest signature verification disabled.");
            return acceptance(ours, score);
        }

        let demand = match their.get_property::<String>(DEMAND_MANIFEST_PROPERTY) {
            Ok(manifest_encoded) => match decode_manifest(&manifest_encoded) {
                Ok(manifest) => DemandWithManifest {
                    demand: their,
                    manifest_encoded,
                    manifest,
                },
                Err(e) => return rejection(format!("invalid manifest: {:?}", e)),
            },
            Err(Error::NoKey(_)) => return acceptance(ours, score),
            Err(e) => return rejection(format!("invalid manifest type: {:?}", e)),
        };

        if demand.has_signature() {
            match demand.verify_signature(&self.keystore) {
                Ok(()) => acceptance(ours, score),
                Err(err) => rejection(format!("failed to verify manifest signature: {}", err)),
            }
        } else if demand.requires_signature(&self.whitelist_matcher) {
            rejection("manifest requires signature but it has none".to_string())
        } else {
            log::trace!("No signature required. No signature provided.");
            acceptance(ours, score)
        }
    }

    fn control_event(&mut self, _component: &str, params: Value) -> anyhow::Result<Value> {
        if let Some(event) = serde_json::from_value::<UpdateKeystore>(params.clone()) {
            self.trusted_keys = Keystore::load(event.keystore_path)
                .map_err(|e| anyhow!("Failed to load keystore file: {e}"))?;
        } else if let Some(event) = serde_json::from_value::<UpdateWhitelist>(params) {
            self.whitelist_matcher = DomainsMatcher::load_or_create(&event.whitelist_path)
                .map_err(|e| anyhow!("Failed to load keystore file: {e}"))?;
        }

        // No return value.
        Ok(Value::Null)
    }
}

// Control event sent to negotiator, that should cause Keystore update.
#[derive(Message, Clone, Debug, Serialize, Deserialize)]
#[rtype(result = "anyhow::Result<serde_json::Value>")]
pub struct UpdateKeystore {
    pub keystore_path: PathBuf,
}

// Control event sent to negotiator, that should cause whitelist update.
#[derive(Message, Clone, Debug, Serialize, Deserialize)]
#[rtype(result = "anyhow::Result<serde_json::Value>")]
pub struct UpdateWhitelist {
    pub whitelist_path: PathBuf,
}

impl ManifestSignature {
    pub fn new(config: serde_yaml::Value) -> anyhow::Result<ManifestSignature> {
        let config: PolicyConfig = serde_yaml::from_value(config)?;
        Ok(ManifestSignature::from(config))
    }
}

pub fn policy_from_env() -> anyhow::Result<NegotiatorConfig> {
    // Empty command line arguments, because we want to use ENV fallback
    // or default values if ENV variables are not set.
    let config = PolicyConfig::from_iter_safe(&[""])?;
    Ok(NegotiatorConfig {
        name: "ManifestSignature".to_string(),
        load_mode: LoadMode::StaticLib {
            library: "ya-provider".to_string(),
        },
        params: serde_yaml::to_value(&config)?,
    })
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

        let whitelist_matcher = config.domain_patterns.matchers.clone();
        let keystore = config.trusted_keys.unwrap_or_default();
        ManifestSignature {
            enabled,
            keystore,
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

    fn requires_signature(&self, whitelist_matcher: &DomainsMatcher) -> bool {
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
                let matcher = whitelist_matcher;
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
}

fn rejection(message: String) -> anyhow::Result<NegotiationResult> {
    Ok(NegotiationResult::Reject {
        reason: RejectReason::new(message),
        is_final: true,
    })
}

fn acceptance(offer: ProposalView, score: Score) -> anyhow::Result<NegotiationResult> {
    Ok(NegotiationResult::Ready {
        proposal: offer,
        score,
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
