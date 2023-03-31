use anyhow::anyhow;
use serde_yaml;
use std::ops::Not;
use std::path::PathBuf;
use std::time::Duration;
use structopt::StructOpt;

use ya_manifest_utils::policy::{Match, Policy, PolicyConfig};
use ya_manifest_utils::{
    decode_manifest, Feature, CAPABILITIES_PROPERTY, DEMAND_MANIFEST_CERT_PROPERTY,
    DEMAND_MANIFEST_NODE_DESCRIPTOR_PROPERTY, DEMAND_MANIFEST_PROPERTY,
    DEMAND_MANIFEST_SIG_ALGORITHM_PROPERTY, DEMAND_MANIFEST_SIG_PROPERTY,
};
use ya_negotiators::agreement::Error;
use ya_negotiators::component::{
    NegotiationResult, NegotiatorComponentMut, NegotiatorFactory, NegotiatorMut, ProposalView,
    RejectReason, Score,
};
use ya_negotiators::factory::{LoadMode, NegotiatorConfig};

use crate::market::config::AgentNegotiatorsConfig;
use crate::rules::{ManifestSignatureProps, RulesManager};
use crate::startup_config::FileMonitor;

pub struct ManifestSignature {
    enabled: bool,
    rules_manager: RulesManager,

    rulestore_monitor: FileMonitor,
    keystore_monitor: FileMonitor,
    whitelist_monitor: FileMonitor,
}

impl NegotiatorComponentMut for ManifestSignature {
    fn negotiate_step(
        &mut self,
        their: &ProposalView,
        ours: ProposalView,
        score: Score,
    ) -> anyhow::Result<NegotiationResult> {
        if self.enabled.not() {
            log::trace!("Manifest verification disabled.");
            return acceptance(ours, score);
        }

        let (manifest, manifest_encoded) =
            match their.get_property::<String>(DEMAND_MANIFEST_PROPERTY) {
                Ok(manifest_encoded) => match decode_manifest(&manifest_encoded) {
                    Ok(manifest) => (manifest, manifest_encoded),
                    Err(e) => return rejection(format!("invalid manifest: {e:?}")),
                },
                Err(Error::NoKey(_)) => return acceptance(ours, score),
                Err(e) => return rejection(format!("invalid manifest type: {e:?}")),
            };

        let manifest_sig = {
            if their
                .get_property::<String>(DEMAND_MANIFEST_SIG_PROPERTY)
                .is_ok()
            {
                let sig = their.get_property::<String>(DEMAND_MANIFEST_SIG_PROPERTY)?;
                log::trace!("sig_hex: {sig}");
                let sig_alg: String = their.get_property(DEMAND_MANIFEST_SIG_ALGORITHM_PROPERTY)?;
                log::trace!("sig_alg: {sig_alg}");
                let cert: String = their.get_property(DEMAND_MANIFEST_CERT_PROPERTY)?;
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

        let node_descriptor = their
            .get_property::<String>(DEMAND_MANIFEST_NODE_DESCRIPTOR_PROPERTY)
            .ok();

        if manifest.is_outbound_requested() {
            match self.rules_manager.check_outbound_rules(
                manifest,
                their.issuer,
                manifest_sig,
                node_descriptor,
            ) {
                crate::rules::CheckRulesResult::Accept => acceptance(ours, score),
                crate::rules::CheckRulesResult::Reject(msg) => rejection(msg),
            }
        } else {
            log::trace!("Outbound is not requested.");
            acceptance(ours, score)
        }
    }

    fn shutdown(&mut self, _timeout: Duration) -> anyhow::Result<()> {
        self.shutdown();
        Ok(())
    }
}

pub fn policy_from_env() -> anyhow::Result<NegotiatorConfig> {
    // Empty command line arguments, because we want to use ENV fallback
    // or default values if ENV variables are not set.
    let policy = PolicyConfig::from_iter_safe(&[""])?;
    Ok(NegotiatorConfig {
        name: "ManifestSignature".to_string(),
        load_mode: LoadMode::StaticLib {
            library: "ya-provider".to_string(),
        },
        params: serde_yaml::to_value(&policy)?,
    })
}

impl NegotiatorFactory<ManifestSignature> for ManifestSignature {
    type Type = NegotiatorMut;

    fn new(
        _name: &str,
        config: serde_yaml::Value,
        agent_env: serde_yaml::Value,
        _workdir: PathBuf,
    ) -> anyhow::Result<ManifestSignature> {
        let config: PolicyConfig = serde_yaml::from_value(config)?;
        let agent_env: AgentNegotiatorsConfig = serde_yaml::from_value(agent_env)?;
        ManifestSignature::from(config, agent_env)
    }
}

impl ManifestSignature {
    pub fn from(policy: PolicyConfig, agent_env: AgentNegotiatorsConfig) -> anyhow::Result<Self> {
        let policies = policy.policy_set();
        let properties = policy.trusted_property_map();

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

        let rules_manager = RulesManager::load_or_create(
            &agent_env.rules_file,
            &agent_env.whitelist_file,
            &agent_env.cert_dir,
        )
        .map_err(|e| anyhow!("Failed to load RulesManager: {e}"))?;

        let (rulestore_monitor, keystore_monitor, whitelist_monitor) = rules_manager
            .spawn_file_monitors()
            .map_err(|e| anyhow!("Failed to spawn rules monitors: {e}"))?;

        Ok(ManifestSignature {
            enabled,
            rules_manager,
            rulestore_monitor,
            keystore_monitor,
            whitelist_monitor,
        })
    }

    fn shutdown(&mut self) {
        self.rulestore_monitor.stop();
        self.keystore_monitor.stop();
        self.whitelist_monitor.stop();
    }
}

impl Drop for ManifestSignature {
    fn drop(&mut self) {
        self.shutdown();
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
    use tempdir::TempDir;

    fn build_policy<S: AsRef<str>>(args: S) -> (ManifestSignature, TempDir) {
        let tempdir = TempDir::new("test_dir").unwrap();
        let rules_file = tempdir.path().join("rules.json");
        let whitelist_file = tempdir.path().join("whitelist.json");
        let cert_dir = tempdir.path().join("cert_dir");

        let arguments = shlex::split(args.as_ref()).expect("failed to parse arguments");

        let policy = serde_yaml::to_value(PolicyConfig::from_iter(arguments)).unwrap();
        let agent_env = serde_yaml::to_value(AgentNegotiatorsConfig {
            rules_file,
            whitelist_file,
            cert_dir,
        })
        .unwrap();

        (
            ManifestSignature::new("", policy, agent_env, PathBuf::new()).unwrap(),
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
