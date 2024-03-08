use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::rules::store::Rulestore;
use crate::rules::CheckRulesResult;

use golem_certificate::schemas::certificate::Fingerprint;
use serde_json::Value;
use ya_client_model::NodeId;
use ya_manifest_utils::CompositeKeystore;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RestrictConfig {
    pub enabled: bool,
    #[serde(default)]
    pub identity: HashSet<NodeId>,
    #[serde(default)]
    pub certified: HashSet<String>,
}

/// Temporary struct storing minimal context for blacklist and allow-only
/// list rules validation.
#[derive(Clone)]
pub struct RestrictRule {
    rulestore: Rulestore,
    keystore: CompositeKeystore,
}

pub trait AllowOnlyValidator {
    fn check_allow_only_rule(
        &self,
        requestor_id: NodeId,
        node_descriptor: Option<serde_json::Value>,
    ) -> CheckRulesResult;
}

pub trait BlacklistValidator {
    fn check_blacklist_rule(
        &self,
        requestor_id: NodeId,
        node_descriptor: Option<serde_json::Value>,
    ) -> CheckRulesResult;
}

impl RestrictConfig {
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn check_identity(&self, requestor_id: NodeId) -> bool {
        self.identity.contains(&requestor_id)
    }

    pub fn contains_certified(&self, cert_id: &Fingerprint) -> bool {
        self.certified.contains(cert_id)
    }
}

impl RestrictRule {
    pub fn new(rulestore: Rulestore, keystore: CompositeKeystore) -> Self {
        Self {
            rulestore,
            keystore,
        }
    }

    pub fn check_certified(
        &self,
        config: &RestrictConfig,
        requestor_id: NodeId,
        node_descriptor: serde_json::Value,
    ) -> anyhow::Result<bool> {
        let node_descriptor = self
            .keystore
            .verify_node_descriptor(node_descriptor)
            .map_err(|e| anyhow!("Allow-only rule: {e}"))?;

        if requestor_id != node_descriptor.node_id {
            return Err(anyhow!(
                "Node ids mismatch. requestor node_id: {requestor_id} but cert node_id: {}",
                node_descriptor.node_id
            ));
        }

        for cert_id in node_descriptor.certificate_chain_fingerprints.iter() {
            if config.contains_certified(cert_id) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

impl AllowOnlyValidator for RestrictRule {
    fn check_allow_only_rule(
        &self,
        requestor_id: NodeId,
        node_descriptor: Option<Value>,
    ) -> CheckRulesResult {
        let config = &self.rulestore.config.read().unwrap().allow_only;
        if config.is_enabled() {
            if config.check_identity(requestor_id) {
                return CheckRulesResult::Accept;
            }

            if let Some(node_descriptor) = node_descriptor {
                return match self.check_certified(config, requestor_id, node_descriptor) {
                    Ok(true) => CheckRulesResult::Accept,
                    Ok(false) => CheckRulesResult::Reject(format!(
                        "Allow-only rule: Requestor [{requestor_id}] is not on the allow-only list"
                    )),
                    Err(e) => CheckRulesResult::Reject(format!(
                        "Allow-only rule: Requestor [{requestor_id}] rejected due to suspicious behavior: {e} "
                    )),
                };
            }

            CheckRulesResult::Reject("Requestor is not on the allow-only list".to_string())
        } else {
            log::trace!("Checking rules: allow-only rule is disabled.");
            CheckRulesResult::Accept
        }
    }
}

impl BlacklistValidator for RestrictRule {
    fn check_blacklist_rule(
        &self,
        requestor_id: NodeId,
        node_descriptor: Option<Value>,
    ) -> CheckRulesResult {
        let config = &self.rulestore.config.read().unwrap().blacklist;
        if config.is_enabled() {
            if config.check_identity(requestor_id) {
                return CheckRulesResult::Reject(format!(
                    "Requestor's NodeId is on the blacklist: {requestor_id}"
                ));
            }

            if let Some(node_descriptor) = node_descriptor {
                return match self.check_certified(&config, requestor_id, node_descriptor) {
                    Ok(true) =>  CheckRulesResult::Reject(format!(
                        "Requestor's certificate is on the blacklist: {requestor_id}"
                    )),
                    Ok(false) => CheckRulesResult::Accept,
                    Err(e) => CheckRulesResult::Reject(format!(
                        "Blacklist rule: Requestor [{requestor_id}] rejected due to suspicious behavior: {e} "
                    )),
                };
            }

            CheckRulesResult::Accept
        } else {
            log::trace!("Checking rules: blacklist rule is disabled.");
            CheckRulesResult::Accept
        }
    }
}

/// Custom implementation to ensure that the rule is disabled by default.
impl Default for RestrictConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            identity: HashSet::new(),
            certified: HashSet::new(),
        }
    }
}
