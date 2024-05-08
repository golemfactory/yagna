pub mod outbound;
pub mod restrict;
mod store;

use crate::rules::outbound::{CertRule, Mode, OutboundRules};
use crate::rules::restrict::{AllowOnly, Blacklist, RestrictRule, RuleAccessor};
use crate::rules::store::Rulestore;
use crate::startup_config::FileMonitor;

use anyhow::{bail, Result};
use golem_certificate::schemas::certificate::Fingerprint;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::{
    convert::TryFrom,
    path::{Path, PathBuf},
};
use strum_macros::Display;

use ya_client_model::NodeId;
use ya_manifest_utils::keystore::{AddParams, AddResponse};
use ya_manifest_utils::{
    keystore::{x509_keystore::X509CertData, Cert, Keystore},
    matching::domain::{DomainPatterns, DomainWhitelistState, DomainsMatcher},
    CompositeKeystore, OutboundAccess,
};

#[derive(Clone)]
pub struct RulesManager {
    pub rulestore: Rulestore,
    pub keystore: CompositeKeystore,
    whitelist: DomainWhitelistState,
    whitelist_file: PathBuf,
}

impl RulesManager {
    pub fn load_or_create(
        rules_file: &Path,
        whitelist_file: &Path,
        cert_dir: &Path,
    ) -> Result<Self> {
        let keystore = CompositeKeystore::load(&cert_dir.into())?;

        let patterns = DomainPatterns::load_or_create(whitelist_file)?;
        let whitelist = DomainWhitelistState::try_new(patterns)?;

        let rulestore = Rulestore::load_or_create(rules_file)?;

        let manager = Self {
            whitelist_file: whitelist_file.to_path_buf(),
            rulestore,
            keystore,
            whitelist,
        };

        manager.remove_dangling_rules()?;

        Ok(manager)
    }

    pub fn blacklist(&self) -> RestrictRule<Blacklist> {
        RestrictRule::<Blacklist>::new(self.rulestore.clone(), self.keystore.clone())
    }

    pub fn allow_only(&self) -> RestrictRule<AllowOnly> {
        RestrictRule::<AllowOnly>::new(self.rulestore.clone(), self.keystore.clone())
    }

    pub fn outbound(&self) -> OutboundRules {
        OutboundRules::new(
            self.rulestore.clone(),
            self.keystore.clone(),
            self.whitelist.clone(),
        )
    }

    /// TODO: Compatibility method. `self.outbound()` interface should be used instead.
    pub fn check_outbound_rules(
        &self,
        access: OutboundAccess,
        requestor_id: NodeId,
        manifest_sig: Option<ManifestSignatureProps>,
        node_descriptor: Option<serde_json::Value>,
    ) -> CheckRulesResult {
        self.outbound()
            .check_outbound_rules(access, requestor_id, manifest_sig, node_descriptor)
    }

    /// TODO: function should be able to distinguish x509 and golem certificates and import
    ///       both types. Currently, it's only importing golem certificates.
    pub fn import_certs(&mut self, import_cert: &Path) -> Result<Vec<Fingerprint>> {
        let AddResponse {
            invalid,
            leaf_cert_ids,
            duplicated,
            ..
        } = self.keystore.add_golem_cert(&AddParams {
            certs: vec![import_cert.to_path_buf()],
        })?;

        for cert_path in invalid {
            log::error!("Failed to import Golem certificates from: {cert_path:?}.");
        }

        self.keystore.reload()?;

        for cert in duplicated {
            log::info!(
                "Certificate is already in keystore: {}, `{}`",
                cert.subject(),
                cert.id()
            );
        }

        Ok(leaf_cert_ids)
    }

    pub fn set_audited_payload_mode(&self, cert_id: String, mode: Mode) -> Result<()> {
        let cert_id = {
            let certs: Vec<Cert> = self
                .keystore
                .list()
                .into_iter()
                .filter(|cert| cert.id().starts_with(&cert_id))
                .collect();

            if certs.is_empty() {
                bail!(
                    "Setting Audited-Payload mode {mode} failed: No cert id: {cert_id} found in keystore"
                );
            } else if certs.len() > 1 {
                bail!(
                    "Setting Audited-Payload mode {mode} failed: Cert id: {cert_id} isn't unique"
                );
            } else {
                let cert = &certs[0];
                match cert {
                    Cert::X509(X509CertData { id, .. }) => id.clone(),
                    Cert::Golem { .. } => bail!(
                        "Failed to set Audited Payload mode for Golem certificate {cert_id}. Audited Payload mode is not yet supported for Golem certificates."
                    ),
                }
            }
        };

        self.rulestore
            .config
            .write()
            .unwrap()
            .outbound
            .audited_payload
            .insert(
                cert_id.clone(),
                CertRule {
                    mode: mode.clone(),
                    description: "".into(),
                },
            );
        log::trace!("Added Audited-Payload rule for cert_id: {cert_id} with mode: {mode}");

        self.rulestore.save()
    }

    pub fn add_rules_information_to_certs(&self, certs: Vec<Cert>) -> Vec<CertWithRules> {
        let cfg = self.rulestore.config.read().unwrap();

        certs
            .into_iter()
            .map(|cert| {
                let mut outbound_rules: Vec<Rule> = Vec::new();
                if cfg.outbound.partner.contains_key(&cert.id()) {
                    outbound_rules.push(Rule::Outbound(OutboundRule::Partner));
                }
                if cfg.outbound.audited_payload.contains_key(&cert.id()) {
                    outbound_rules.push(Rule::Outbound(OutboundRule::AuditedPayload));
                }
                if cfg.blacklist.certified.contains(&cert.id()) {
                    outbound_rules.push(Rule::Blacklist);
                }
                if cfg.allow_only.certified.contains(&cert.id()) {
                    outbound_rules.push(Rule::AllowOnly);
                }
                CertWithRules {
                    cert,
                    rules: outbound_rules,
                }
            })
            .collect()
    }

    pub fn set_partner_mode(&self, cert_id: String, mode: Mode) -> Result<()> {
        let cert_id = {
            let certs: Vec<Cert> = self
                .keystore
                .list()
                .into_iter()
                .filter(|cert| cert.id().starts_with(&cert_id))
                .collect();

            if certs.is_empty() {
                bail!(
                    "Setting Partner mode {mode} failed: No cert id: {cert_id} found in keystore"
                );
            } else if certs.len() > 1 {
                bail!("Setting Partner mode {mode} failed: Cert id: {cert_id} isn't unique");
            } else {
                let cert = &certs[0];
                match cert {
                    Cert::X509(_) => bail!(
                        "Failed to set partner mode for certificate {cert_id}. Partner mode can be set only for Golem certificate."
                    ),
                    Cert::Golem { id, .. } => id.clone(),
                }
            }
        };

        self.rulestore
            .config
            .write()
            .unwrap()
            .outbound
            .partner
            .insert(
                cert_id.clone(),
                CertRule {
                    mode: mode.clone(),
                    description: "".into(),
                },
            );
        log::trace!("Added Partner rule for cert_id: {cert_id} with mode: {mode}");

        self.rulestore.save()
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<()> {
        log::debug!("Setting outbound enabled: {enabled}");
        self.rulestore.config.write().unwrap().outbound.enabled = enabled;

        self.rulestore.save()
    }

    pub fn set_everyone_mode(&self, mode: Mode) -> Result<()> {
        log::debug!("Setting outbound everyone mode: {mode}");
        self.rulestore.config.write().unwrap().outbound.everyone = mode;

        self.rulestore.save()
    }

    pub fn spawn_file_monitors(&self) -> Result<(FileMonitor, FileMonitor, FileMonitor)> {
        let rulestore_monitor = {
            let manager = self.clone();
            let handler = move |p: PathBuf| {
                // Reload also keystore to avoid file-monitor race when doing `import-cert`
                match manager.keystore.reload() {
                    Ok(()) => {
                        log::info!("Trusted keystore updated because rulestore changed");
                    }
                    Err(e) => {
                        log::warn!("Error updating trusted keystore when rulestore changed: {e}")
                    }
                }

                match manager.rulestore.reload() {
                    Ok(()) => {
                        log::info!("rulestore updated from {}", p.display());

                        if let Err(e) = manager.remove_dangling_rules() {
                            log::warn!("Error removing unnecessary rules: {e}");
                        }
                    }
                    Err(e) => log::warn!("Error updating rulestore from {}: {e}", p.display()),
                }
            };
            FileMonitor::spawn(&self.rulestore.path, FileMonitor::on_modified(handler))?
        };

        let keystore_monitor = {
            let manager = self.clone();
            let handler = move |p: PathBuf| match manager.keystore.reload() {
                Ok(()) => {
                    log::info!("Trusted keystore updated from {}", p.display());

                    if let Err(e) = manager.remove_dangling_rules() {
                        log::warn!("Error removing unnecessary rules: {e}");
                    }
                }
                Err(e) => log::warn!("Error updating trusted keystore from {}: {e}", p.display()),
            };
            FileMonitor::spawn(self.keystore.cert_dir(), FileMonitor::on_modified(handler))?
        };

        let whitelist_monitor = {
            let state = self.whitelist.clone();
            let handler = move |p: PathBuf| match DomainPatterns::load(&p) {
                Ok(patterns) => {
                    match DomainsMatcher::try_from(&patterns) {
                        Ok(matcher) => {
                            *state.matchers.write().unwrap() = matcher;
                            *state.patterns.lock().unwrap() = patterns;

                            log::info!("Whitelist updated from {}", p.display());
                        }
                        Err(e) => log::error!("Failed to update domain whitelist: {e}"),
                    };
                }
                Err(e) => log::warn!("Error updating whitelist from {}: {e}", p.display()),
            };
            FileMonitor::spawn(
                self.whitelist_file.clone(),
                FileMonitor::on_modified(handler),
            )?
        };

        Ok((rulestore_monitor, keystore_monitor, whitelist_monitor))
    }

    /// Removes all outbound rules that are not matching any certificate in keystore.
    fn remove_dangling_rules(&self) -> Result<()> {
        let keystore_cert_ids = self.keystore.list_ids();
        let removed_rules = self.remove_rules_not_matching_any_cert(&keystore_cert_ids);

        if removed_rules.is_empty() {
            return Ok(());
        }
        if !removed_rules.partner.is_empty() {
            log::warn!("Because Keystore didn't have appropriate certs, following Outbound Partner rules were removed: {:?}", removed_rules.partner);
        }
        if !removed_rules.audited_payload.is_empty() {
            log::warn!("Because Keystore didn't have appropriate certs, following Outbound Audited-Payload rules were removed: {:?}", removed_rules.audited_payload);
        }
        if !removed_rules.blacklist.is_empty() {
            log::warn!("Because Keystore didn't have appropriate certs, following Blacklist rules were removed: {:?}", removed_rules.blacklist);
        }
        if !removed_rules.allow_only.is_empty() {
            log::warn!("Because Keystore didn't have appropriate certs, following AllowOnly rules were removed: {:?}", removed_rules.allow_only);
        }
        self.rulestore.save()
    }

    fn remove_rules_not_matching_any_cert(&self, cert_ids: &[String]) -> RemovedRules {
        RemovedRules {
            partner: self.outbound().remove_unmatched_partner_rules(cert_ids),
            audited_payload: self
                .outbound()
                .remove_unmatched_audited_payload_rules(cert_ids),
            blacklist: remove_unmatched_certs(self.blacklist(), cert_ids),
            allow_only: remove_unmatched_certs(self.allow_only(), cert_ids),
        }
    }
}

fn remove_unmatched_certs<G: RuleAccessor>(
    rule: RestrictRule<G>,
    keystore_certs: &[Fingerprint],
) -> Vec<Fingerprint> {
    rule.list_certs()
        .into_iter()
        .filter(|cert| !keystore_certs.contains(cert))
        .inspect(|cert| {
            rule.remove_certified_rule(cert).ok();
        })
        .collect()
}

type RemovedRulesIds = Vec<String>;

struct RemovedRules {
    partner: RemovedRulesIds,
    audited_payload: RemovedRulesIds,
    blacklist: RemovedRulesIds,
    allow_only: RemovedRulesIds,
}

impl RemovedRules {
    fn is_empty(&self) -> bool {
        self.partner.is_empty()
            && self.audited_payload.is_empty()
            && self.blacklist.is_empty()
            && self.allow_only.is_empty()
    }
}

pub struct ManifestSignatureProps {
    pub sig: String,
    pub sig_alg: String,
    pub cert: String,
    pub manifest_encoded: String,
}

pub enum CheckRulesResult {
    Accept,
    Reject(String),
}

#[derive(PartialEq, Eq)]
pub struct CertWithRules {
    pub cert: Cert,
    pub rules: Vec<Rule>,
}

impl CertWithRules {
    pub fn format_rules(&self) -> String {
        self.rules.iter().map(|r| r.to_string()).join(" | ")
    }
}

#[derive(PartialEq, Eq, derive_more::Display, Debug, Clone, Serialize, Deserialize)]
pub enum Rule {
    #[display(fmt = "Outbound-{}", _0)]
    Outbound(OutboundRule),
    Blacklist,
    AllowOnly,
}

#[derive(PartialEq, Eq, Display, Debug, Clone, Serialize, Deserialize)]
pub enum OutboundRule {
    Partner,
    AuditedPayload,
    Everyone,
}
