use crate::startup_config::FileMonitor;
use anyhow::{anyhow, bail, Result};
use golem_certificate::schemas::permissions::Permissions;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    convert::TryFrom,
    fs::OpenOptions,
    io::BufReader,
    ops::Not,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};
use structopt::StructOpt;
use strum::{Display, EnumString, EnumVariantNames};
use url::Url;
use ya_client_model::NodeId;
use ya_manifest_utils::{
    keystore::{x509_keystore::X509CertData, Cert, Keystore},
    matching::{
        domain::{DomainPatterns, DomainWhitelistState, DomainsMatcher},
        Matcher,
    },
    CompositeKeystore, OutboundAccess,
};

#[derive(Clone)]
pub struct RulesManager {
    pub rulestore: Rulestore,
    pub keystore: CompositeKeystore,
    pub cert_dir: PathBuf,
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
            cert_dir: cert_dir.to_path_buf(),
            rulestore,
            keystore,
            whitelist,
        };

        manager.remove_dangling_rules()?;

        Ok(manager)
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
                let mut outbound_rules: Vec<OutboundRule> = Vec::new();
                if cfg.outbound.partner.contains_key(&cert.id()) {
                    outbound_rules.push(OutboundRule::Partner);
                }
                if cfg.outbound.audited_payload.contains_key(&cert.id()) {
                    outbound_rules.push(OutboundRule::AuditedPayload);
                }
                CertWithRules {
                    cert,
                    outbound_rules,
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
            let handler = move |p: PathBuf| match manager.rulestore.reload() {
                Ok(()) => {
                    log::info!("rulestore updated from {}", p.display());

                    if let Err(e) = manager.remove_dangling_rules() {
                        log::warn!("Error removing unnecessary rules: {e}");
                    }
                }
                Err(e) => log::warn!("Error updating rulestore from {}: {e}", p.display()),
            };
            FileMonitor::spawn(&self.rulestore.path, FileMonitor::on_modified(handler))?
        };

        let keystore_monitor = {
            let cert_dir = self.cert_dir.clone();
            let manager = self.clone();
            let handler = move |p: PathBuf| match manager.keystore.reload(&cert_dir) {
                Ok(()) => {
                    log::info!("Trusted keystore updated from {}", p.display());

                    if let Err(e) = manager.remove_dangling_rules() {
                        log::warn!("Error removing unnecessary rules: {e}");
                    }
                }
                Err(e) => log::warn!("Error updating trusted keystore from {}: {e}", p.display()),
            };
            FileMonitor::spawn(self.cert_dir.clone(), FileMonitor::on_modified(handler))?
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

        if removed_rules.partner.is_empty() && removed_rules.partner.is_empty() {
            return Ok(());
        }
        if !removed_rules.partner.is_empty() {
            log::warn!("Because Keystore didn't have appropriate certs, following Outbound Partner rules were removed: {:?}", removed_rules.partner);
        }
        if !removed_rules.audited_payload.is_empty() {
            log::warn!("Because Keystore didn't have appropriate certs, following Outbound Audited-Payload rules were removed: {:?}", removed_rules.audited_payload);
        }
        self.rulestore.save()
    }

    fn remove_rules_not_matching_any_cert(&self, cert_ids: &[String]) -> RemovedRules {
        let mut rulestore = self.rulestore.config.write().unwrap();
        let removed_partner_rules =
            remove_rules_not_matching_any_cert(&mut rulestore.outbound.partner, cert_ids);
        let removed_audited_payload_rules =
            remove_rules_not_matching_any_cert(&mut rulestore.outbound.audited_payload, cert_ids);
        RemovedRules {
            partner: removed_partner_rules,
            audited_payload: removed_audited_payload_rules,
        }
    }

    fn check_everyone_rule(&self, access: &OutboundAccess) -> Result<()> {
        let mode = &self.rulestore.config.read().unwrap().outbound.everyone;

        self.check_mode(mode, access)
            .map_err(|e| anyhow!("Everyone {e}"))
    }

    fn check_audited_payload_rule(
        &self,
        access: &OutboundAccess,
        manifest_sig: Option<ManifestSignatureProps>,
    ) -> Result<()> {
        if let Some(props) = manifest_sig {
            let cert_chain_ids = self
                .keystore
                .verifier(&props.cert)?
                .with_alg(&props.sig_alg)
                .verify(&props.manifest_encoded, &props.sig)
                .map_err(|e| anyhow!("Audited-Payload rule: {e}"))?;

            let rulestore_config = self.rulestore.config.read().unwrap();
            // Rule set for certificate closes to leaf takes precedence
            // either:
            // 1. a rule is set directly for the cert
            // 2. an issuer for the certificate is in keystore and there's a rule for this cert
            for cert_id in cert_chain_ids.iter().rev() {
                if let Some(rule) = rulestore_config.outbound.audited_payload.get(cert_id) {
                    return self
                        .check_mode(&rule.mode, access)
                        .map_err(|e| anyhow!("Audited-Payload {e}"));
                }
            }

            Err(anyhow!(
                "Audited-Payload rule whole chain of cert_ids is not trusted: {:?}",
                cert_chain_ids
            ))
        } else {
            Err(anyhow!("Audited-Payload rule requires manifest signature"))
        }
    }

    fn check_partner_rule(
        &self,
        access: &OutboundAccess,
        node_descriptor: Option<serde_json::Value>,
        requestor_id: NodeId,
    ) -> Result<()> {
        let node_descriptor =
            node_descriptor.ok_or_else(|| anyhow!("Partner rule requires node descriptor"))?;

        let node_descriptor = self
            .keystore
            .verify_node_descriptor(node_descriptor)
            .map_err(|e| anyhow!("Partner {e}"))?;

        if requestor_id != node_descriptor.node_id {
            return Err(anyhow!(
                "Partner rule nodes mismatch. requestor node_id: {requestor_id} but cert node_id: {}",
                node_descriptor.node_id
            ));
        }

        self::verify_golem_permissions(&node_descriptor.permissions, access)
            .map_err(|e| anyhow!("Partner {e}"))?;

        for cert_id in node_descriptor.certificate_chain_fingerprints.iter() {
            if let Some(rule) = self
                .rulestore
                .config
                .read()
                .unwrap()
                .outbound
                .partner
                .get(cert_id)
            {
                return self
                    .check_mode(&rule.mode, access)
                    .map_err(|e| anyhow!("Partner {e}"));
            }
        }
        Err(anyhow!(
            "Partner rule whole chain of cert_ids is not trusted: {:?}",
            node_descriptor.certificate_chain_fingerprints
        ))
    }

    fn check_mode(&self, mode: &Mode, access: &OutboundAccess) -> Result<()> {
        log::trace!("Checking mode: {mode}");

        match mode {
            Mode::All => Ok(()),
            Mode::Whitelist => {
                if self.whitelist_matching(access) {
                    log::trace!("Whitelist matched");

                    Ok(())
                } else {
                    Err(anyhow!("rule didn't match whitelist"))
                }
            }
            Mode::None => Err(anyhow!("rule is disabled")),
        }
    }

    pub fn check_outbound_rules(
        &self,
        access: OutboundAccess,
        requestor_id: NodeId,
        manifest_sig: Option<ManifestSignatureProps>,
        node_descriptor: Option<serde_json::Value>,
    ) -> CheckRulesResult {
        if self.rulestore.is_outbound_disabled() {
            log::trace!("Checking rules: outbound is disabled.");

            return CheckRulesResult::Reject("outbound is disabled".into());
        }

        let (accepts, rejects): (Vec<_>, Vec<_>) = vec![
            self.check_everyone_rule(&access),
            self.check_audited_payload_rule(&access, manifest_sig),
            self.check_partner_rule(&access, node_descriptor, requestor_id),
        ]
        .into_iter()
        .partition_result();

        let reject_msg = extract_rejected_message(rejects);

        log::info!("Following rules didn't match: {reject_msg}");

        if accepts.is_empty().not() {
            CheckRulesResult::Accept
        } else {
            CheckRulesResult::Reject(format!("Outbound rejected because: {reject_msg}"))
        }
    }

    fn whitelist_matching(&self, outbound_access: &OutboundAccess) -> bool {
        match outbound_access {
            ya_manifest_utils::OutboundAccess::Urls(urls) => {
                let matcher = self.whitelist.matchers.read().unwrap();
                let non_whitelisted_urls: Vec<&str> = urls
                    .iter()
                    .flat_map(Url::host_str)
                    .filter(|domain| matcher.matches(domain).not())
                    .collect();

                if non_whitelisted_urls.is_empty() {
                    true
                } else {
                    log::debug!(
                        "Whitelist. Non whitelisted URLs: {:?}",
                        non_whitelisted_urls
                    );
                    false
                }
            }
            ya_manifest_utils::OutboundAccess::Unrestricted => false,
        }
    }
}

fn remove_rules_not_matching_any_cert(
    rules: &mut HashMap<String, CertRule>,
    cert_ids: &[String],
) -> Vec<String> {
    let mut deleted_rules = vec![];
    rules.retain(|cert_id, _| {
        cert_ids
            .contains(cert_id)
            .not()
            .then(|| deleted_rules.push(cert_id.clone()))
            .is_none()
    });
    deleted_rules
}

type RemovedRulesIds = Vec<String>;

struct RemovedRules {
    partner: RemovedRulesIds,
    audited_payload: RemovedRulesIds,
}

fn verify_golem_permissions(
    cert_permissions: &Permissions,
    outbound_access: &OutboundAccess,
) -> Result<()> {
    match cert_permissions {
        Permissions::All => Ok(()),
        Permissions::Object(details) => match &details.outbound {
            Some(outbound_permissions) => match outbound_permissions {
                golem_certificate::schemas::permissions::OutboundPermissions::Unrestricted => {
                    Ok(())
                }
                golem_certificate::schemas::permissions::OutboundPermissions::Urls(
                    permitted_urls,
                ) => {
                    match outbound_access {
                        OutboundAccess::Urls(requested_urls) => {
                            for requested_url in requested_urls {
                                if permitted_urls.contains(&requested_url).not() {
                                    anyhow::bail!("Partner rule forbidden url requested: {requested_url}");
                                }
                            }
                        },
                        OutboundAccess::Unrestricted => anyhow::bail!("Manifest tries to use Unrestricted access, but certificate allows only for specific urls"),
                    }
                    Ok(())
                }
            },
            None => anyhow::bail!("No outbound permissions"),
        },
    }
}

fn extract_rejected_message(rules_checks: Vec<anyhow::Error>) -> String {
    rules_checks
        .iter()
        .fold(String::new(), |s, c| s + &c.to_string() + " ; ")
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

#[derive(Clone, Debug)]
pub struct Rulestore {
    pub path: PathBuf,
    pub config: Arc<RwLock<RulesConfig>>,
}

impl Rulestore {
    pub fn load_or_create(rules_file: &Path) -> Result<Self> {
        if rules_file.exists() {
            log::debug!("Loading rule from: {}", rules_file.display());
            let file = OpenOptions::new().read(true).open(rules_file)?;

            Ok(Self {
                config: Arc::new(serde_json::from_reader(BufReader::new(file))?),
                path: rules_file.to_path_buf(),
            })
        } else {
            log::debug!("Creating default Rules configuration");
            let config = Default::default();

            let store = Self {
                config: Arc::new(RwLock::new(config)),
                path: rules_file.to_path_buf(),
            };
            store.save()?;

            Ok(store)
        }
    }

    fn save(&self) -> Result<()> {
        log::debug!("Saving RuleStore to: {}", self.path.display());
        Ok(std::fs::write(
            &self.path,
            serde_json::to_string_pretty(&*self.config.read().unwrap())?,
        )?)
    }

    pub fn reload(&self) -> Result<()> {
        log::debug!("Reloading RuleStore from: {}", self.path.display());
        let new_rule_store = Self::load_or_create(&self.path)?;

        self.replace(new_rule_store);

        Ok(())
    }

    fn replace(&self, other: Self) {
        let store = std::mem::take(&mut (*other.config.write().unwrap()));

        *self.config.write().unwrap() = store;
    }

    pub fn print(&self) -> Result<()> {
        println!(
            "{}",
            serde_json::to_string_pretty(&*self.config.read().unwrap())?
        );

        Ok(())
    }

    pub fn is_outbound_disabled(&self) -> bool {
        self.config.read().unwrap().outbound.enabled.not()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RulesConfig {
    pub outbound: OutboundConfig,
}

impl Default for RulesConfig {
    fn default() -> Self {
        Self {
            outbound: OutboundConfig {
                enabled: true,
                everyone: Mode::None,
                audited_payload: HashMap::new(),
                partner: HashMap::new(),
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct OutboundConfig {
    pub enabled: bool,
    pub everyone: Mode,
    #[serde(default)]
    pub audited_payload: HashMap<String, CertRule>,
    #[serde(default)]
    pub partner: HashMap<String, CertRule>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CertRule {
    pub mode: Mode,
    pub description: String,
}

#[derive(
    StructOpt,
    Clone,
    Debug,
    Serialize,
    Deserialize,
    Eq,
    PartialEq,
    Display,
    EnumString,
    EnumVariantNames,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum Mode {
    All,
    None,
    Whitelist,
}

#[derive(PartialEq, Eq, Display, Debug, Clone)]
pub enum OutboundRule {
    Partner,
    AuditedPayload,
    Everyone,
}

#[derive(PartialEq, Eq)]
pub struct CertWithRules {
    pub cert: Cert,
    pub outbound_rules: Vec<OutboundRule>,
}

impl CertWithRules {
    pub fn format_outbound_rules(&self) -> String {
        self.outbound_rules
            .iter()
            .map(|r| r.to_string())
            .join(" | ")
    }
}
