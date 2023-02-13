use anyhow::{anyhow, bail};
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::fs::File;
use std::hash::Hash;
use std::ops::Not;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, RwLock};

use openssl::hash::MessageDigest;
use openssl::pkey::{PKey, Public};
use openssl::sign::Verifier;
use openssl::x509::store::{X509Store, X509StoreBuilder};
use openssl::x509::{X509ObjectRef, X509Ref, X509StoreContext, X509VerifyResult, X509};
use serde::{Deserialize, Serialize};
use structopt::StructOpt;
use strum::{Display, EnumIter, EnumString, EnumVariantNames, IntoEnumIterator, VariantNames};

use crate::golem_certificate::{verify_golem_certificate, GolemCertificate};
use crate::util::{cert_to_id, format_permissions, CertBasicDataVisitor, X509Visitor};

pub(crate) const PERMISSIONS_FILE: &str = "cert-permissions.json";

/// Policy configuration
#[derive(StructOpt, Clone, Debug, Default)]
pub struct PolicyConfig {
    /// Disable policy components
    #[structopt(
        long,
        env,
        parse(try_from_str),
        possible_values = Policy::VARIANTS,
        case_insensitive = true,
    )]
    pub policy_disable_component: Vec<Policy>,
    /// Whitelist property names (optionally filtered by value)
    // e.g.
    //  POLICY_TRUST_PROPERTY="prop1=1|2,prop2=3|4|5,prop3"
    //  POLICY_TRUST_PROPERTY=prop4
    #[structopt(
        long,
        env,
        parse(try_from_str = parse_property_match),
    )]
    pub policy_trust_property: Vec<(String, Match)>,
}

impl PolicyConfig {
    pub fn policy_set(&self) -> HashSet<Policy> {
        if self.policy_disable_component.contains(&Policy::All) {
            Default::default()
        } else {
            let mut components: HashSet<_> = Policy::iter().collect();
            components.retain(|c| self.policy_disable_component.contains(c).not());
            components
        }
    }

    #[inline]
    pub fn trusted_property_map(&self) -> HashMap<String, Match> {
        self.policy_trust_property.iter().cloned().collect()
    }
}

#[non_exhaustive]
#[derive(
    Clone, Copy, Debug, Hash, Eq, PartialEq, EnumIter, EnumVariantNames, EnumString, Display,
)]
#[strum(serialize_all = "snake_case")]
pub enum Policy {
    All,
    ManifestSignatureValidation,
    ManifestCompliance,
    ManifestInetUrlCompliance,
    ManifestScriptCompliance,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Display)]
pub enum Match {
    All,
    Values(Vec<String>),
}

impl FromStr for Match {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // use '|' instead of ',' to support multi-value
        // environment variables
        let values: Vec<_> = s.split('|').map(|v| v.trim().to_string()).collect();
        Ok(if values.is_empty() {
            Match::All
        } else {
            Match::Values(values)
        })
    }
}

#[derive(
    Clone,
    Copy,
    Debug,
    EnumIter,
    EnumVariantNames,
    EnumString,
    Display,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
)]
#[strum(serialize_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum CertPermissions {
    /// Allows all permissions (including permissions created in future)
    All,
    /// Certificate is allowed to sign Payload Manifest requiring Outbound Network Traffic feature.
    OutboundManifest,
    /// Permissions signed by this certificate will not be verified.
    UnverifiedPermissionsChain,
}

#[derive(Clone, Default)]
pub struct PermissionsManager {
    permissions: HashMap<String, Vec<CertPermissions>>,
}

pub struct CertStore {
    store: X509Store,
    permissions: PermissionsManager,
}

#[derive(Clone)]
pub struct Keystore {
    inner: Arc<RwLock<CertStore>>,
}

impl Default for Keystore {
    fn default() -> Self {
        let store = X509StoreBuilder::new().expect("SSL works").build();
        Self {
            inner: CertStore::new(store, Default::default()),
        }
    }
}

impl CertStore {
    pub fn new(store: X509Store, permissions: PermissionsManager) -> Arc<RwLock<CertStore>> {
        Arc::new(RwLock::new(CertStore { store, permissions }))
    }
}

impl Keystore {
    /// Reads DER or PEM certificates (or PEM certificate stacks) from `cert-dir` and creates new `X509Store`.
    pub fn load(cert_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&cert_dir)?;
        let permissions = PermissionsManager::load(&cert_dir).map_err(|e| {
            anyhow!(
                "Failed to load permissions file: {}, {e}",
                cert_dir.as_ref().display()
            )
        })?;

        let mut store = X509StoreBuilder::new()?;
        let cert_dir = std::fs::read_dir(cert_dir)?;
        for dir_entry in cert_dir {
            let cert = dir_entry?.path();
            if let Err(e) = Self::load_file(&mut store, &cert) {
                log::debug!(
                    "Skipping '{}' while loading a keystore. Error: {e}",
                    cert.display()
                );
            }
        }
        let store = CertStore::new(store.build(), permissions);
        Ok(Keystore { inner: store })
    }

    pub fn reload(&self, cert_dir: impl AsRef<Path>) -> anyhow::Result<()> {
        let keystore = Keystore::load(&cert_dir)?;
        self.replace(keystore);
        Ok(())
    }

    fn replace(&self, other: Keystore) {
        let store = {
            let mut inner = other.inner.write().unwrap();
            std::mem::replace(
                &mut (*inner),
                CertStore {
                    store: X509StoreBuilder::new().unwrap().build(),
                    permissions: Default::default(),
                },
            )
        };
        let mut inner = self.inner.write().unwrap();
        *inner = store;
    }

    /// Decodes byte64 `sig`, verifies `cert`and reads its pub key,
    /// prepares digest using `sig_alg`, verifies `data` using `sig` and pub key.
    pub fn verify_signature(
        &self,
        cert: impl AsRef<str>,
        sig: impl AsRef<str>,
        sig_alg: impl AsRef<str>,
        data: impl AsRef<str>,
    ) -> anyhow::Result<()> {
        let sig = crate::decode_data(sig)?;

        let pkey = self.verify_cert(cert)?;

        let msg_digest = MessageDigest::from_name(sig_alg.as_ref())
            .ok_or_else(|| anyhow::anyhow!("Unknown signature algorithm: {}", sig_alg.as_ref()))?;
        let mut verifier = Verifier::new(msg_digest, pkey.as_ref())?;
        if !(verifier.verify_oneshot(&sig, data.as_ref().as_bytes())?) {
            return Err(anyhow::anyhow!("Invalid signature"));
        }
        Ok(())
    }

    pub fn verify_golem_certificate(&self, cert: &str) -> anyhow::Result<GolemCertificate> {
        verify_golem_certificate(cert)
            .map_err(|e| anyhow!("verification of golem certificate failed: {e}"))
    }

    pub fn certs_ids(&self) -> anyhow::Result<HashSet<String>> {
        let inner = self.inner.read().unwrap();
        let mut ids = HashSet::new();
        for cert in inner.store.objects() {
            if let Some(cert) = cert.x509() {
                let id = crate::util::cert_to_id(cert)?;
                ids.insert(id);
            }
        }

        //TODO it will be deleted when keystore will handle golem certs properly
        ids.insert("all".into());
        ids.insert("outbound".into());
        ids.insert("expired".into());
        ids.insert("invalid-signature".into());
        ids.insert("outbound-urls".into());
        ids.insert("no-permissions".into());

        Ok(ids)
    }

    pub(crate) fn visit_certs<T: CertBasicDataVisitor>(
        &self,
        visitor: &mut X509Visitor<T>,
    ) -> anyhow::Result<()> {
        let inner = self.inner.read().unwrap();
        for cert in inner.store.objects().iter().flat_map(X509ObjectRef::x509) {
            visitor.accept(cert, &inner.permissions)?;
        }
        Ok(())
    }

    fn load_file(store: &mut X509StoreBuilder, cert: &PathBuf) -> anyhow::Result<()> {
        for cert in crate::util::parse_cert_file(cert)? {
            store.add_cert(cert)?
        }
        Ok(())
    }

    fn verify_cert<S: AsRef<str>>(&self, cert: S) -> anyhow::Result<PKey<Public>> {
        let cert_chain = Self::decode_cert_chain(cert)?;
        let store = self
            .inner
            .read()
            .map_err(|err| anyhow::anyhow!("Err: {}", err.to_string()))?;
        let cert = match cert_chain.last().map(Clone::clone) {
            Some(cert) => cert,
            None => bail!("Unable to verify certificate. No certificate."),
        };
        let mut cert_stack = openssl::stack::Stack::new()?;
        for cert in cert_chain {
            cert_stack.push(cert).unwrap();
        }
        let mut ctx = X509StoreContext::new()?;
        if !(ctx.init(&store.store, &cert, &cert_stack, |ctx| ctx.verify_cert())?) {
            bail!("Invalid certificate");
        }
        Ok(cert.public_key()?)
    }

    pub fn verify_permissions<S: AsRef<str>>(
        &self,
        cert: S,
        required: Vec<CertPermissions>,
    ) -> anyhow::Result<()> {
        if required.contains(&CertPermissions::All) {
            bail!("`All` permissions shouldn't be required.")
        }

        if required.is_empty() {
            return Ok(());
        }

        let cert_chain = Self::decode_cert_chain(cert)?;
        // Demands do not contain certificates permissions
        // so only first certificate in chain signer permissions are verified.
        let cert = match cert_chain.first() {
            Some(cert) => cert,
            None => bail!("Unable to verify certificate permissions. No certificate."),
        };
        let issuer = self.find_issuer(cert)?;

        self.has_permissions(&issuer, &required)
    }

    fn get_permissions(&self, cert: &X509Ref) -> anyhow::Result<Vec<CertPermissions>> {
        let store = self
            .inner
            .read()
            .map_err(|err| anyhow::anyhow!("RwLock error: {}", err.to_string()))?;
        Ok(store.permissions.get(cert))
    }

    fn has_permissions(
        &self,
        cert: &X509Ref,
        required: &Vec<CertPermissions>,
    ) -> anyhow::Result<()> {
        let cert_permissions = self.get_permissions(cert)?;

        if cert_permissions.contains(&CertPermissions::All)
            && (!required.contains(&CertPermissions::UnverifiedPermissionsChain)
                || (cert_permissions.contains(&CertPermissions::UnverifiedPermissionsChain)
                    && required.contains(&CertPermissions::UnverifiedPermissionsChain)))
        {
            return Ok(());
        }

        if required
            .iter()
            .all(|permission| cert_permissions.contains(permission))
        {
            return Ok(());
        }

        bail!(
            "Not sufficient permissions. Required: `{}`, but has only: `{}`",
            format_permissions(required),
            format_permissions(&cert_permissions)
        )
    }

    fn find_issuer(&self, cert: &X509) -> anyhow::Result<X509> {
        let store = self
            .inner
            .read()
            .map_err(|err| anyhow::anyhow!("RwLock error: {}", err.to_string()))?;
        store
            .store
            .objects()
            .iter()
            .filter_map(|cert| cert.x509())
            .map(|cert| cert.to_owned())
            .find(|trusted| trusted.issued(cert) == X509VerifyResult::OK)
            .ok_or_else(|| anyhow!("Issuer certificate not found in keystore"))
    }

    fn decode_cert_chain<S: AsRef<str>>(cert: S) -> anyhow::Result<Vec<X509>> {
        let cert = crate::decode_data(cert)?;
        Ok(match X509::from_der(&cert) {
            Ok(cert) => vec![cert],
            Err(_) => X509::stack_from_pem(&cert)?,
        })
    }

    pub fn permissions_manager(&self) -> PermissionsManager {
        self.inner.read().unwrap().permissions.clone()
    }
}

impl PermissionsManager {
    pub fn load(cert_dir: impl AsRef<Path>) -> anyhow::Result<PermissionsManager> {
        let path = cert_dir.as_ref().join(PERMISSIONS_FILE);
        let content = match std::fs::read_to_string(path) {
            Ok(content) if !content.is_empty() => content,
            _ => return Ok(Default::default()),
        };
        let permissions = serde_json::from_str(&content)?;
        Ok(PermissionsManager { permissions })
    }

    pub fn set(&mut self, cert: &str, mut permissions: Vec<CertPermissions>) {
        if permissions.contains(&CertPermissions::All) {
            let supports_unverified_permissions =
                permissions.contains(&CertPermissions::UnverifiedPermissionsChain);
            permissions.clear();
            permissions.push(CertPermissions::All);
            if supports_unverified_permissions {
                permissions.push(CertPermissions::UnverifiedPermissionsChain);
            }
        }
        self.permissions.insert(cert.to_string(), permissions);
    }

    pub fn set_x509(
        &mut self,
        cert: &X509,
        permissions: Vec<CertPermissions>,
    ) -> anyhow::Result<()> {
        let id = cert_to_id(cert)?;
        self.set(&id, permissions);
        Ok(())
    }

    pub fn set_many(
        &mut self,
        // With slice I would need add `openssl` dependency directly to ya-rovider.
        #[allow(clippy::ptr_arg)] certs: &Vec<X509>,
        permissions: Vec<CertPermissions>,
        whole_chain: bool,
    ) {
        let certs = match whole_chain {
            false => Self::leaf_certs(certs),
            true => certs.clone(),
        };

        for cert in certs {
            if let Err(e) = self.set_x509(&cert, permissions.clone()) {
                log::error!(
                    "Failed to set permissions for certificate {:?}. {e}",
                    cert.subject_name()
                );
            }
        }
    }

    /// If we don't have this certificate registered, it means it has no permissions,
    /// so empty vector is returned.
    pub fn get(&self, cert: &X509Ref) -> Vec<CertPermissions> {
        let id = match cert_to_id(cert) {
            Ok(id) => id,
            Err(_) => return vec![],
        };
        self.permissions.get(&id).cloned().unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let mut file = File::create(&path.join(PERMISSIONS_FILE))?;
        Ok(serde_json::to_writer_pretty(&mut file, &self.permissions)?)
    }

    fn leaf_certs(certs: &[X509]) -> Vec<X509> {
        if certs.len() == 1 {
            // when there is 1 cert it is a leaf cert
            return certs.to_vec();
        }
        certs
            .iter()
            .cloned()
            .filter(|cert| {
                !certs
                    .iter()
                    .any(|cert2| cert.issued(cert2) == X509VerifyResult::OK)
            })
            .collect()
    }
}

impl std::fmt::Debug for Keystore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Keystore")
    }
}

fn parse_property_match(input: &str) -> anyhow::Result<(String, Match)> {
    let mut split = input.splitn(2, '=');
    let property = split
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing property name"))?
        .to_string();
    let values = match split.next() {
        Some(s) => Match::from_str(s)?,
        None => Match::All,
    };
    Ok((property, values))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_config<S: AsRef<str>>(args: S) -> PolicyConfig {
        let arguments = shlex::split(args.as_ref()).expect("failed to parse arguments");
        PolicyConfig::from_iter(arguments)
    }

    #[test]
    fn policy_config() {
        let config = build_config("TEST");
        assert_eq!(config.policy_disable_component, Vec::default());
        assert_eq!(config.policy_trust_property, Vec::default());

        let config = build_config(
            "TEST \
            --policy-trust-property property",
        );
        assert_eq!(config.policy_disable_component, Vec::default());
        assert_eq!(
            config.policy_trust_property,
            vec![("property".to_string(), Match::All)]
        );

        let config = build_config(
            "TEST \
            --policy-disable-component all \
            --policy-trust-property property=value1|value2",
        );
        assert_eq!(config.policy_disable_component, vec![Policy::All]);
        assert_eq!(
            config.policy_trust_property,
            vec![(
                "property".to_string(),
                Match::Values(vec!["value1".to_string(), "value2".to_string()])
            )]
        );
    }
}
