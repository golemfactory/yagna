use std::{
    convert::TryFrom,
    fs::OpenOptions,
    io::BufReader,
    ops::Not,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use structopt::StructOpt;
use strum::Display;
use url::Url;
use ya_manifest_utils::{
    matching::{
        domain::{DomainPatterns, DomainWhitelistState, DomainsMatcher},
        Matcher,
    },
    AppManifest, Keystore,
};

use crate::startup_config::FileMonitor;

//TODO Rafał Default is set only for unit test purposes in manifest negotiator ...
#[derive(Clone, Debug, Default)]
pub struct RulesManager {
    pub rules_file: PathBuf,
    pub config: Rulestore,
    whitelist_file: PathBuf,
    cert_dir: PathBuf,
    //TODO Rafał Move files into keystore and whiteliststate
    keystore: Keystore,
    whitelist: DomainWhitelistState,
}

#[derive(Clone, Debug, Default)]
pub struct Rulestore {
    pub rules_file: PathBuf,
    pub config: Arc<RwLock<RulesConfig>>,
}

impl Rulestore {
    pub fn load_or_create(rules_file: &Path) -> Result<Self> {
        if rules_file.exists() {
            log::debug!("Loading rule from: {}", rules_file.display());
            let file = OpenOptions::new().read(true).open(rules_file)?;

            Ok(Self {
                config: Arc::new(serde_json::from_reader(BufReader::new(file))?),
                rules_file: rules_file.to_path_buf(),
            })
        } else {
            log::debug!("Creating default Rules configuration");
            let config = Default::default();

            let store = Self {
                config: Arc::new(RwLock::new(config)),
                rules_file: rules_file.to_path_buf(),
            };
            store.save()?;

            Ok(store)
        }
    }

    fn save(&self) -> Result<()> {
        log::debug!("Saving RuleStore to: {}", self.rules_file.display());
        Ok(std::fs::write(
            &self.rules_file,
            serde_json::to_string_pretty(&*self.config.read().unwrap())?,
        )?)
    }

    pub fn reload(&self) -> Result<()> {
        log::debug!("Reloading RuleStore from: {}", self.rules_file.display());
        let new_rule_store = Self::load_or_create(&self.rules_file)?;

        self.replace(new_rule_store);

        Ok(())
    }

    fn replace(&self, other: Self) {
        let store = std::mem::take(&mut (*other.config.write().unwrap()));

        *self.config.write().unwrap() = store;
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<()> {
        log::debug!("Setting outbound enabled: {enabled}");
        self.config.write().unwrap().outbound.enabled = enabled;

        self.save()
    }

    pub fn set_everyone_mode(&self, mode: Mode) -> Result<()> {
        log::debug!("Setting outbound everyone mode: {mode}");
        self.config.write().unwrap().outbound.everyone = mode;

        self.save()
    }

    pub fn set_default_audited_payload_mode(&self, mode: Mode) -> Result<()> {
        log::debug!("Setting outbound audited_payload default mode: {mode}");
        self.config
            .write()
            .unwrap()
            .outbound
            .audited_payload
            .default
            .mode = mode;

        self.save()
    }

    pub fn print(&self) -> Result<()> {
        println!(
            "{}",
            serde_json::to_string_pretty(&*self.config.read().unwrap())?
        );

        Ok(())
    }
}

impl RulesManager {
    pub fn load_or_create(
        rules_file: &Path,
        whitelist_file: &Path,
        cert_dir: &Path,
    ) -> Result<Self> {
        let keystore = Keystore::load(cert_dir)?;
        let patterns = DomainPatterns::load_or_create(whitelist_file)?;
        let whitelist = DomainWhitelistState::try_new(patterns)?;

        let config = Rulestore::load_or_create(rules_file)?;

        Ok(Self {
            rules_file: rules_file.to_path_buf(),
            whitelist_file: whitelist_file.to_path_buf(),
            cert_dir: cert_dir.to_path_buf(),
            config,
            keystore,
            whitelist,
        })
    }

    pub fn spawn_file_monitors(&self) -> Result<(FileMonitor, FileMonitor, FileMonitor)> {
        let path = self.rules_file.clone();
        let rulestore = self.config.clone();
        let handler = move |p: PathBuf| match rulestore.reload() {
            Ok(()) => {
                log::info!("rulestore updated from {}", p.display());
            }
            Err(e) => log::warn!("Error updating rulestore from {}: {e}", p.display()),
        };
        let rulestore_monitor = FileMonitor::spawn(path, FileMonitor::on_modified(handler))?;

        let cert_dir = self.cert_dir.clone();
        let keystore = self.keystore.clone();
        let handler = move |p: PathBuf| match keystore.reload(&cert_dir) {
            Ok(()) => {
                log::info!("Trusted keystore updated from {}", p.display());
            }
            Err(e) => log::warn!("Error updating trusted keystore from {}: {e}", p.display()),
        };
        let keystore_monitor =
            FileMonitor::spawn(self.cert_dir.clone(), FileMonitor::on_modified(handler))?;

        let state = self.whitelist.clone();
        let handler = move |p: PathBuf| match DomainPatterns::load(&p) {
            Ok(patterns) => {
                match DomainsMatcher::try_from(&patterns) {
                    Ok(matcher) => {
                        *state.matchers.write().unwrap() = matcher;
                        *state.patterns.lock().unwrap() = patterns;
                    }
                    Err(err) => log::error!("Failed to update domain whitelist: {err}"),
                };
            }
            Err(e) => log::warn!(
                "Error updating whitelist configuration from {:?}: {:?}",
                p,
                e
            ),
        };
        let whitelist_monitor = FileMonitor::spawn(
            self.whitelist_file.clone(),
            FileMonitor::on_modified(handler),
        )?;

        Ok((rulestore_monitor, keystore_monitor, whitelist_monitor))
    }

    pub fn check_outbound_rules(
        &self,
        manifest: AppManifest,
        manifest_sig_props: Option<ManifestSignatureProps>,
    ) -> CheckRulesResult {
        //TODO Rafał config config
        let cfg = self.config.config.read().unwrap();

        if cfg.outbound.enabled.not() {
            log::trace!("Outbound is disabled.");
            return CheckRulesResult::Reject("outbound is disabled".into());
        }

        match cfg.outbound.everyone {
            Mode::All => {
                log::trace!("Everyone is allowed for outbound");

                return CheckRulesResult::Accept;
            }
            Mode::Whitelist => {
                return if self.whitelist_matching(&manifest) {
                    log::trace!("Everyone Whitelist matched");
                    CheckRulesResult::Accept
                } else {
                    CheckRulesResult::Reject("Everyone Whitelist does not match".into())
                }
            }
            Mode::None => log::trace!("Everyone rule is disabled"),
        }

        if let Some(manifest_sig_props) = manifest_sig_props {
            //Check audited-payload Rule
            if let Err(e) = self.keystore.verify_signature(
                manifest_sig_props.cert,
                manifest_sig_props.sig,
                manifest_sig_props.sig_alg,
                manifest_sig_props.manifest_encoded,
            ) {
                return CheckRulesResult::Reject(format!(
                    "failed to verify manifest signature: {e}"
                ));
            }
            //TODO Add verification of permission tree when they will be included in x509 (as there will be in both Rules)

            match cfg.outbound.audited_payload.default.mode {
                Mode::All => {
                    log::trace!("Audited-Payload rule set to all");
                    CheckRulesResult::Accept
                }
                Mode::Whitelist => {
                    if self.whitelist_matching(&manifest) {
                        log::trace!("Audited-Payload whitelist matched");
                        CheckRulesResult::Accept
                    } else {
                        CheckRulesResult::Reject("Audited-Payload whitelist doesn't match".into())
                    }
                }
                Mode::None => CheckRulesResult::Reject("Audited-Payload rule is disabled".into()),
            }
        } else {
            //Check partner Rule
            CheckRulesResult::Reject("Didn't match any Rules".into())
        }
    }

    fn whitelist_matching(&self, manifest: &AppManifest) -> bool {
        if let Some(urls) = manifest
            .comp_manifest
            .as_ref()
            .and_then(|comp| comp.net.as_ref())
            .and_then(|net| net.inet.as_ref())
            .and_then(|inet| inet.out.as_ref())
            .and_then(|out| out.urls.as_ref())
        {
            let matcher = self.whitelist.matchers.read().unwrap();
            let non_whitelisted_urls: Vec<&str> = urls
                .iter()
                .flat_map(Url::host_str)
                .filter(|domain| matcher.matches(domain).not())
                .collect();
            if non_whitelisted_urls.is_empty() {
                log::debug!("Every URL on whitelist");
                return true;
            }
            log::debug!(
                "Whitelist. Non whitelisted URLs: {:?}",
                non_whitelisted_urls
            );
            false
        } else {
            log::debug!("No URLs to check");
            true
        }
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

#[derive(StructOpt, Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Display)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    All,
    None,
    Whitelist,
}
