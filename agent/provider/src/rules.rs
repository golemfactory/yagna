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
    keystore::{Cert, Keystore},
    matching::{
        domain::{DomainPatterns, DomainWhitelistState, DomainsMatcher},
        Matcher,
    },
    policy::CertPermissions,
    AppManifest, CompositeKeystore,
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

    pub fn remove_dangling_rules(&self) -> Result<()> {
        let mut deleted_partner_rules = vec![];

        let keystore_certs = self.keystore.list_ids();

        self.rulestore
            .config
            .write()
            .unwrap()
            .outbound
            .partner
            .retain(|cert_id, _| {
                keystore_certs
                    .contains(cert_id)
                    .not()
                    .then(|| deleted_partner_rules.push(cert_id.clone()))
                    .is_none()
            });

        if deleted_partner_rules.is_empty() {
            Ok(())
        } else {
            log::warn!("Because Keystore didn't have appriopriate certs, following Partner rules were removed: {:?}", deleted_partner_rules);

            self.rulestore.save()
        }
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

    pub fn set_default_audited_payload_mode(&self, mode: Mode) -> Result<()> {
        log::debug!("Setting outbound audited_payload default mode: {mode}");
        self.rulestore
            .config
            .write()
            .unwrap()
            .outbound
            .audited_payload
            .default
            .mode = mode;

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

    fn check_everyone_rule(&self, manifest: &AppManifest) -> Result<()> {
        let mode = &self.rulestore.config.read().unwrap().outbound.everyone;

        self.check_mode(mode, manifest)
            .map_err(|e| anyhow!("Everyone {e}"))
    }

    fn check_audited_payload_rule(
        &self,
        manifest: &AppManifest,
        manifest_sig: Option<ManifestSignatureProps>,
    ) -> Result<()> {
        if let Some(props) = manifest_sig {
            self.keystore
                .verify_x509_signature(
                    props.cert.clone(),
                    props.sig,
                    props.sig_alg,
                    props.manifest_encoded,
                )
                .map_err(|e| anyhow!("Audited-Payload rule: {e}"))?;

            //TODO Add verification of permission tree when they will be included in x509 (as there will be in both Rules)
            self.verify_permissions(&props.cert)
                .map_err(|e| anyhow!("Audited-Payload rule: {e}"))?;

            let mode = &self
                .rulestore
                .config
                .read()
                .unwrap()
                .outbound
                .audited_payload
                .default
                .mode;

            self.check_mode(mode, manifest)
                .map_err(|e| anyhow!("Audited-Payload {e}"))
        } else {
            Err(anyhow!("Audited-Payload rule requires manifest signature"))
        }
    }

    fn check_partner_rule(
        &self,
        manifest: &AppManifest,
        node_descriptor: Option<String>,
        requestor_id: NodeId,
    ) -> Result<()> {
        let node_descriptor =
            node_descriptor.ok_or_else(|| anyhow!("Partner rule requires node descriptor"))?;

        let node_descriptor = self
            .keystore
            .verify_node_descriptor(&node_descriptor)
            .map_err(|e| anyhow!("Partner {e}"))?;

        if requestor_id != node_descriptor.node_id {
            return Err(anyhow!(
                "Partner rule nodes mismatch. requestor node_id: {requestor_id} but cert node_id: {}",
                node_descriptor.node_id
            ));
        }

        self::verify_golem_permissions(
            &node_descriptor.permissions,
            &manifest.get_outbound_requested_urls(),
        )
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
                    .check_mode(&rule.mode, manifest)
                    .map_err(|e| anyhow!("Partner {e}"));
            }
        }
        Err(anyhow!(
            "Partner rule whole chain of cert_ids is not trusted: {:?}",
            node_descriptor.certificate_chain_fingerprints
        ))
    }

    fn check_mode(&self, mode: &Mode, manifest: &AppManifest) -> Result<()> {
        log::trace!("Checking mode: {mode}");

        match mode {
            Mode::All => Ok(()),
            Mode::Whitelist => {
                if self.whitelist_matching(manifest) {
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
        manifest: AppManifest,
        requestor_id: NodeId,
        manifest_sig: Option<ManifestSignatureProps>,
        node_descriptor: Option<String>,
    ) -> CheckRulesResult {
        if self.rulestore.is_outbound_disabled() {
            log::trace!("Checking rules: outbound is disabled.");

            return CheckRulesResult::Reject("outbound is disabled".into());
        }

        let (accepts, rejects): (Vec<_>, Vec<_>) = vec![
            self.check_everyone_rule(&manifest),
            self.check_audited_payload_rule(&manifest, manifest_sig),
            self.check_partner_rule(&manifest, node_descriptor, requestor_id),
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

    fn whitelist_matching(&self, manifest: &AppManifest) -> bool {
        let urls = manifest.get_outbound_requested_urls();
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

    fn verify_permissions(&self, cert: &str) -> Result<()> {
        let required = vec![CertPermissions::OutboundManifest];
        self.keystore.verify_permissions(cert, required)
    }
}

fn verify_golem_permissions(cert_permissions: &Permissions, requested_urls: &[Url]) -> Result<()> {
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
                    for requested_url in requested_urls {
                        if permitted_urls.contains(requested_url).not() {
                            anyhow::bail!("Partner rule forbidden url requested: {requested_url}");
                        }
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
                audited_payload: CertRules {
                    default: CertRule {
                        mode: Mode::All,
                        description: "Default setting".into(),
                    },
                },
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
    pub audited_payload: CertRules,
    #[serde(default)]
    pub partner: HashMap<String, CertRule>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CertRules {
    pub default: CertRule,
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
