use crate::rules::store::Rulestore;
use crate::rules::{CheckRulesResult, ManifestSignatureProps};
use anyhow::anyhow;
use golem_certificate::schemas::permissions::Permissions;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ops::Not;
use structopt::StructOpt;
use strum_macros::{Display, EnumString, EnumVariantNames};
use url::Url;
use ya_client_model::NodeId;
use ya_manifest_utils::keystore::Keystore;
use ya_manifest_utils::matching::domain::DomainWhitelistState;
use ya_manifest_utils::matching::Matcher;
use ya_manifest_utils::{CompositeKeystore, OutboundAccess};

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

/// Temporary struct storing minimal context for outbound rules validation.
#[derive(Clone)]
pub struct OutboundRules {
    rulestore: Rulestore,
    keystore: CompositeKeystore,
    whitelist: DomainWhitelistState,
}
impl OutboundRules {
    pub fn new(
        rulestore: Rulestore,
        keystore: CompositeKeystore,
        whitelist: DomainWhitelistState,
    ) -> Self {
        Self {
            rulestore,
            keystore,
            whitelist,
        }
    }
    pub fn remove_unmatched_partner_rules(&self, cert_ids: &[String]) -> Vec<String> {
        let mut rulestore = self.rulestore.config.write().unwrap();
        remove_rules_not_matching_any_cert(&mut rulestore.outbound.partner, cert_ids)
    }

    pub fn remove_unmatched_audited_payload_rules(&self, cert_ids: &[String]) -> Vec<String> {
        let mut rulestore = self.rulestore.config.write().unwrap();
        remove_rules_not_matching_any_cert(&mut rulestore.outbound.partner, cert_ids)
    }

    pub fn config(&self) -> OutboundConfig {
        self.rulestore.config.read().unwrap().outbound.clone()
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

    fn check_everyone_rule(&self, access: &OutboundAccess) -> anyhow::Result<()> {
        let mode = &self.rulestore.config.read().unwrap().outbound.everyone;

        self.check_mode(mode, access)
            .map_err(|e| anyhow!("Everyone {e}"))
    }

    fn check_audited_payload_rule(
        &self,
        access: &OutboundAccess,
        manifest_sig: Option<ManifestSignatureProps>,
    ) -> anyhow::Result<()> {
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
    ) -> anyhow::Result<()> {
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

    fn check_mode(&self, mode: &Mode, access: &OutboundAccess) -> anyhow::Result<()> {
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

fn extract_rejected_message(rules_checks: Vec<anyhow::Error>) -> String {
    rules_checks
        .iter()
        .fold(String::new(), |s, c| s + &c.to_string() + " ; ")
}

fn verify_golem_permissions(
    cert_permissions: &Permissions,
    outbound_access: &OutboundAccess,
) -> anyhow::Result<()> {
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
                                if permitted_urls.contains(requested_url).not() {
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

impl Default for OutboundConfig {
    fn default() -> Self {
        OutboundConfig {
            enabled: true,
            everyone: Mode::Whitelist,
            audited_payload: HashMap::new(),
            partner: HashMap::new(),
        }
    }
}
