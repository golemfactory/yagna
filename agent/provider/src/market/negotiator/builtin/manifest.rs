use actix::Message;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_yaml;
use std::ops::Not;
use std::path::PathBuf;
use structopt::StructOpt;

use ya_agreement_utils::{Error, ProposalView};
use ya_manifest_utils::manifest::{
    decode_manifest, Feature, Signature, CAPABILITIES_PROPERTY, DEMAND_MANIFEST_PROPERTY,
    DEMAND_MANIFEST_SIG_PROPERTY,
};
use ya_manifest_utils::policy::{Keystore, Match, Policy, PolicyConfig};
use ya_negotiators::component::{RejectReason, Score};
use ya_negotiators::factory::{LoadMode, NegotiatorConfig};
use ya_negotiators::{NegotiationResult, NegotiatorComponent};

#[derive(Default)]
pub struct ManifestSignature {
    enabled: bool,
    trusted_keys: Keystore,
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
        }

        let manifest = match their.get_property::<String>(DEMAND_MANIFEST_PROPERTY) {
            Err(Error::NoKey(_)) => {
                return Ok(NegotiationResult::Ready {
                    proposal: ours,
                    score,
                })
            }
            Err(e) => return rejection(format!("invalid manifest type: {:?}", e)),
            Ok(s) => match decode_manifest(s) {
                Ok(manifest) => manifest,
                Err(e) => return rejection(format!("invalid manifest: {:?}", e)),
            },
        };

        if manifest.features().is_empty() {
            return Ok(NegotiationResult::Ready {
                proposal: ours,
                score,
            });
        }

        let pub_key = match verify_signature(their) {
            Ok(pub_key) => pub_key,
            Err(e) => return rejection(format!("invalid manifest signature: {:?}", e)),
        };

        if self.trusted_keys.contains(pub_key.as_slice()) {
            Ok(NegotiationResult::Ready {
                proposal: ours,
                score,
            })
        } else {
            rejection("manifest not signed by a trusted authority".to_string())
        }
    }

    fn control_event(&mut self, _component: &str, params: Value) -> anyhow::Result<Value> {
        let event: UpdateKeystore =
            serde_json::from_value(params).map_err(|e| anyhow!("Unrecognized event: {e}"))?;

        self.trusted_keys = Keystore::load(event.keystore_path)
            .map_err(|e| anyhow!("Failed to load keystore file: {e}"))?;

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

        ManifestSignature {
            enabled,
            trusted_keys: config.trusted_keys.unwrap_or_default(),
        }
    }
}

fn rejection(message: String) -> anyhow::Result<NegotiationResult> {
    Ok(NegotiationResult::Reject {
        reason: RejectReason::new(message),
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
