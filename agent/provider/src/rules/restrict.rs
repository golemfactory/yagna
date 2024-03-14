use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::marker::PhantomData;
use std::sync::{RwLockReadGuard, RwLockWriteGuard};

use crate::rules::store::{RulesConfig, Rulestore};
use crate::rules::CheckRulesResult;

use golem_certificate::schemas::certificate::Fingerprint;
use ya_client_model::NodeId;
use ya_manifest_utils::keystore::{Cert, Keystore};
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
pub struct RestrictRule<G: RuleAccessor> {
    rulestore: Rulestore,
    keystore: CompositeKeystore,

    phantom: PhantomData<G>,
}

pub trait RuleAccessor {
    fn write<'a>(guard: &'a mut RwLockWriteGuard<RulesConfig>) -> &'a mut RestrictConfig;
    fn read<'a>(guard: &'a RwLockReadGuard<RulesConfig>) -> &'a RestrictConfig;

    fn rule_name() -> &'static str;
}

pub struct Blacklist {}
pub struct AllowOnly {}

impl RuleAccessor for Blacklist {
    fn write<'a>(guard: &'a mut RwLockWriteGuard<RulesConfig>) -> &'a mut RestrictConfig {
        &mut guard.blacklist
    }

    fn read<'a>(guard: &'a RwLockReadGuard<RulesConfig>) -> &'a RestrictConfig {
        &guard.blacklist
    }

    fn rule_name() -> &'static str {
        "blacklist"
    }
}

impl RuleAccessor for AllowOnly {
    fn write<'a>(guard: &'a mut RwLockWriteGuard<RulesConfig>) -> &'a mut RestrictConfig {
        &mut guard.allow_only
    }

    fn read<'a>(guard: &'a RwLockReadGuard<RulesConfig>) -> &'a RestrictConfig {
        &guard.allow_only
    }

    fn rule_name() -> &'static str {
        "allow-only"
    }
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

    pub fn enable(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn add_certified(&mut self, cert_id: Fingerprint) {
        self.certified.insert(cert_id);
    }

    pub fn add_identity(&mut self, node_id: NodeId) {
        self.identity.insert(node_id);
    }

    pub fn remove_certified(&mut self, cert_id: &Fingerprint) {
        self.certified.remove(cert_id);
    }

    pub fn remove_identity(&mut self, node_id: NodeId) {
        self.identity.remove(&node_id);
    }
}

impl<G> RestrictRule<G>
where
    G: RuleAccessor,
{
    pub fn new(rulestore: Rulestore, keystore: CompositeKeystore) -> Self {
        Self {
            rulestore,
            keystore,
            phantom: Default::default(),
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

    pub fn enable(&self) -> anyhow::Result<()> {
        log::debug!("Enabling {} rule", G::rule_name());
        {
            let mut guard = self.rulestore.config.write().unwrap();
            G::write(&mut guard).enable(true);
        }
        self.rulestore.save()
    }

    pub fn disable(&self) -> anyhow::Result<()> {
        log::debug!("Disabling {} rule", G::rule_name());
        {
            let mut guard = self.rulestore.config.write().unwrap();
            G::write(&mut guard).enable(false);
        }
        self.rulestore.save()
    }

    pub fn add_certified_rule(&self, cert_id: &Fingerprint) -> anyhow::Result<()> {
        let rule_name = G::rule_name();
        let cert_id = {
            let certs: Vec<Cert> = self
                .keystore
                .list()
                .into_iter()
                .filter(|cert| cert.id().starts_with(cert_id))
                .collect();

            if certs.is_empty() {
                bail!("Setting {rule_name} rule failed: No cert id: {cert_id} found in keystore");
            } else if certs.len() > 1 {
                bail!("Setting {rule_name} rule failed: Cert id: {cert_id} isn't unique");
            } else {
                let cert = &certs[0];
                match cert {
                    Cert::X509(_) => bail!(
                        "Failed to set {rule_name} rule for certificate {cert_id}. Only Golem certificate allowed."
                    ),
                    Cert::Golem { id, .. } => id.clone(),
                }
            }
        };

        {
            let mut guard = self.rulestore.config.write().unwrap();
            G::write(&mut guard).add_certified(cert_id.clone());
        }
        log::trace!("Added {rule_name} rule for cert_id: {cert_id}");

        self.rulestore.save()
    }

    pub fn add_identity_rule(&self, node_id: NodeId) -> anyhow::Result<()> {
        {
            let mut guard = self.rulestore.config.write().unwrap();
            G::write(&mut guard).add_identity(node_id);
        }
        self.rulestore.save()
    }

    pub fn remove_certified_rule(&self, cert_id: &Fingerprint) -> anyhow::Result<()> {
        {
            let mut guard = self.rulestore.config.write().unwrap();
            G::write(&mut guard).remove_certified(cert_id);
        }
        self.rulestore.save()
    }

    pub fn remove_identity_rule(&self, node_id: NodeId) -> anyhow::Result<()> {
        {
            let mut guard = self.rulestore.config.write().unwrap();
            G::write(&mut guard).remove_identity(node_id);
        }
        self.rulestore.save()
    }
}

impl<G> AllowOnlyValidator for RestrictRule<G>
where
    G: RuleAccessor,
{
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

impl<G> BlacklistValidator for RestrictRule<G>
where
    G: RuleAccessor,
{
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
