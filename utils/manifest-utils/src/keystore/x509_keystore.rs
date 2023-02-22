use crate::{
    golem_certificate::{verify_golem_certificate, GolemCertificate},
    policy::CertPermissions,
    util::{format_permissions, str_to_short_hash, CertDataVisitor},
};
use anyhow::{anyhow, bail};
use itertools::Itertools;
use openssl::{
    hash::MessageDigest,
    nid::Nid,
    pkey::{PKey, Public},
    sign::Verifier,
    x509::{
        store::{X509Store, X509StoreBuilder},
        X509ObjectRef, X509Ref, X509StoreContext, X509VerifyResult, X509,
    },
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    convert::TryFrom,
    ffi::OsStr,
    fs::{self, DirEntry, File},
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use super::{AddParams, AddResponse, CertData, CommonAddParams, Keystore};

pub(crate) const PERMISSIONS_FILE: &str = "cert-permissions.json";
pub(super) trait X509AddParams {
    fn permissions(&self) -> &Vec<CertPermissions>;
    /// Whether to apply permissions to all certificates from cert directory.
    fn whole_chain(&self) -> bool;
}

impl CertData {
    pub fn create(cert: &X509Ref, permissions: &PermissionsManager) -> anyhow::Result<Self> {
        let mut data = CertData::try_from(cert)?;
        let permissions = permissions.get(cert);
        data.permissions = format_permissions(&permissions);
        Ok(data)
    }
}

pub struct KeystoreLoadResult {
    pub loaded: Vec<X509>,
    pub skipped: Vec<X509>,
}

pub(super) struct X509KeystoreManager {
    keystore: X509Keystore,
    ids: HashSet<String>,
    cert_dir: PathBuf,
}

impl X509KeystoreManager {
    pub fn try_load(cert_dir: &PathBuf) -> anyhow::Result<Self> {
        let keystore = X509Keystore::load(cert_dir)?;
        let ids = keystore.certs_ids()?;
        let cert_dir = cert_dir.clone();
        Ok(Self {
            ids,
            cert_dir,
            keystore,
        })
    }

    /// Copies certificates from given file to `cert-dir` and returns newly added certificates.
    /// Certificates already existing in `cert-dir` are skipped.
    fn add_certs<ADD: X509AddParams + CommonAddParams>(
        &self,
        add: &ADD,
    ) -> anyhow::Result<KeystoreLoadResult> {
        let mut added = HashMap::new();
        let mut skipped = HashMap::new();

        for cert_path in add.certs() {
            let mut new_certs = Vec::new();
            let file_certs = parse_cert_file(cert_path)?;
            if file_certs.is_empty() {
                continue;
            }
            let file_certs_len = file_certs.len();
            for file_cert in file_certs {
                let id = cert_to_id(&file_cert)?;
                if !self.ids.contains(&id) && !added.contains_key(&id) {
                    new_certs.push(file_cert.clone());
                    added.insert(id, file_cert);
                } else {
                    skipped.insert(id, file_cert);
                }
            }
            if file_certs_len == new_certs.len() {
                self.load_as_keychain_file(cert_path)?;
            } else {
                self.load_as_certificate_files(cert_path, new_certs)?;
            }
        }

        self.permissions_manager().set_many(
            &added.values().chain(skipped.values()).cloned().collect(),
            add.permissions(),
            add.whole_chain(),
        );

        Ok(KeystoreLoadResult {
            loaded: added.into_values().collect(),
            skipped: skipped.into_values().collect(),
        })
    }

    pub fn remove_certs(self, ids: &HashSet<String>) -> anyhow::Result<KeystoreRemoveResult> {
        if ids.difference(&self.ids).eq(ids) {
            return Ok(KeystoreRemoveResult::NothingToRemove);
        }
        let mut removed = HashMap::new();

        let cert_dir_entries: Vec<Result<DirEntry, std::io::Error>> =
            std::fs::read_dir(self.cert_dir.clone())?.collect();
        for dir_entry in cert_dir_entries {
            let cert_file = dir_entry?;
            let cert_file = cert_file.path();
            let certs = parse_cert_file(&cert_file)?;
            if certs.is_empty() {
                // No certificates in parsed file.
                continue;
            }
            let mut ids_cert = certs.into_iter().fold(HashMap::new(), |mut certs, cert| {
                if let Ok(id) = cert_to_id(&cert) {
                    certs.insert(id, cert);
                }
                certs
            });
            let mut split_and_skip = false;
            for id in ids.iter() {
                if let Some(cert) = ids_cert.remove(id) {
                    removed.insert(id, cert);
                    split_and_skip = true;
                }
            }
            if split_and_skip {
                let file_stem = get_file_stem(&cert_file).expect("Cannot get file name stem");
                let dot_extension = get_file_extension(&cert_file)
                    .map_or_else(|| String::from(""), |ex| format!(".{ex}"));
                for (id, cert) in ids_cert {
                    let cert = cert.to_pem()?;
                    let mut file_path = self.cert_dir.clone();
                    let filename = format!("{file_stem}.{id}{dot_extension}");
                    file_path.push(filename);
                    fs::write(file_path, cert)?;
                }
                fs::remove_file(cert_file)?;
            }
        }

        let removed: Vec<X509> = removed.into_values().collect();
        Ok(KeystoreRemoveResult::Removed { removed })
    }

    /// Loads keychain file to `cert-dir`
    fn load_as_keychain_file(&self, cert_path: &PathBuf) -> anyhow::Result<()> {
        let file_name = get_file_name(cert_path)
            .ok_or_else(|| anyhow::anyhow!(format!("Cannot get filename of {cert_path:?}")))?;
        let mut new_cert_path = self.cert_dir.clone();
        new_cert_path.push(file_name);
        if new_cert_path.exists() {
            let file_stem = get_file_stem(&new_cert_path).expect("Has to have stem");
            let dot_extension = get_file_extension(&new_cert_path)
                .map(|ex| format!(".{ex}"))
                .unwrap_or_else(|| String::from(""));
            for i in 0..u32::MAX {
                let numbered_filename = format!("{file_stem}.{i}{dot_extension}");
                new_cert_path = self.cert_dir.clone();
                new_cert_path.push(numbered_filename);
                if !new_cert_path.exists() {
                    break;
                }
            }
            if new_cert_path.exists() {
                anyhow::bail!("Unable to load certificate");
            }
        }
        fs::copy(cert_path, new_cert_path)?;
        Ok(())
    }

    /// Loads certificates as individual files to `cert-dir`
    fn load_as_certificate_files(&self, cert_path: &Path, certs: Vec<X509>) -> anyhow::Result<()> {
        let file_stem = get_file_stem(cert_path)
            .ok_or_else(|| anyhow::anyhow!("Cannot get file name stem."))?;
        let dot_extension = get_file_extension(cert_path)
            .map(|ex| format!(".{ex}"))
            .unwrap_or_else(|| String::from(""));
        for cert in certs.into_iter() {
            let id = cert_to_id(&cert)?;
            let mut new_cert_path = self.cert_dir.clone();
            new_cert_path.push(format!("{file_stem}.{id}{dot_extension}"));
            let cert = cert.to_pem()?;
            fs::write(new_cert_path, cert)?;
        }
        Ok(())
    }

    pub fn permissions_manager(&self) -> PermissionsManager {
        self.keystore.permissions_manager()
    }
}

impl Keystore for X509KeystoreManager {
    fn add(&mut self, add: &super::AddParams) -> anyhow::Result<AddResponse> {
        let res = self.add_certs(add)?;
        let permissions_manager = self.keystore.permissions_manager();

        permissions_manager
            .save(&self.cert_dir)
            .map_err(|e| anyhow!("Failed to save permissions file: {e}"))?;
        let added = res
            .loaded
            .into_iter()
            .map(|cert| CertData::create(&cert, &permissions_manager))
            .collect::<anyhow::Result<Vec<CertData>>>()?;
        let duplicated = res
            .skipped
            .into_iter()
            .map(|cert| CertData::create(&cert, &permissions_manager))
            .collect::<anyhow::Result<Vec<CertData>>>()?;
        Ok(AddResponse {
            added,
            skipped: duplicated,
        })
    }

    fn remove(&mut self, remove: &super::RemoveParams) -> anyhow::Result<super::RemoveResponse> {
        todo!()
    }
}

pub struct CertStore {
    store: X509Store,
    permissions: PermissionsManager,
}

#[derive(Clone)]
pub struct X509Keystore {
    inner: Arc<RwLock<CertStore>>,
}

impl Default for X509Keystore {
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

impl X509Keystore {
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
                    "Skipping '{}' while loading a X509Keystore. Error: {e}",
                    cert.display()
                );
            }
        }
        let store = CertStore::new(store.build(), permissions);
        Ok(Self { inner: store })
    }

    pub fn reload(&self, cert_dir: impl AsRef<Path>) -> anyhow::Result<()> {
        let keystore = X509Keystore::load(&cert_dir)?;
        self.replace(keystore);
        Ok(())
    }

    fn replace(&self, other: X509Keystore) {
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
                let id = cert_to_id(cert)?;
                ids.insert(id);
            }
        }

        //TODO it will be deleted when X509Keystore will handle golem certs properly
        ids.insert("all".into());
        ids.insert("outbound".into());
        ids.insert("expired".into());
        ids.insert("invalid-signature".into());
        ids.insert("outbound-urls".into());
        ids.insert("no-permissions".into());

        Ok(ids)
    }

    pub(crate) fn visit_certs<T: CertDataVisitor>(
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
        for cert in parse_cert_file(cert)? {
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
            .ok_or_else(|| anyhow!("Issuer certificate not found in X509Keystore"))
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

fn parse_cert_file(cert: &PathBuf) -> anyhow::Result<Vec<X509>> {
    let extension = get_file_extension(cert);
    let mut cert = File::open(cert)?;
    let mut cert_buffer = Vec::new();
    cert.read_to_end(&mut cert_buffer)?;
    match extension {
        Some(ref der) if der.to_lowercase() == "der" => Ok(vec![X509::from_der(&cert_buffer)?]),
        Some(ref pem) if pem.to_lowercase() == "pem" => Ok(X509::stack_from_pem(&cert_buffer)?),
        _ => {
            // Certificates can have various other extensions like .cer .crt .key (and .key can be both DER and PEM)
            // Initial parsing dictated by `extension` is done because it would improper to parse .pem as a DER
            Ok(X509::stack_from_pem(&cert_buffer)
                .or_else(|_| X509::from_der(&cert_buffer).map(|cert| vec![cert]))?)
        }
    }
}

fn get_file_extension(path: &Path) -> Option<String> {
    path.extension().map(os_str_to_string)
}

fn get_file_name(path: &Path) -> Option<String> {
    path.file_name().map(os_str_to_string)
}

fn get_file_stem(path: &Path) -> Option<String> {
    path.file_stem().map(os_str_to_string)
}

fn os_str_to_string(os_str: &OsStr) -> String {
    os_str.to_string_lossy().to_string()
}

pub fn cert_to_id(cert: &X509Ref) -> anyhow::Result<String> {
    let txt = cert.to_text()?;
    Ok(str_to_short_hash(&txt))
}

pub fn visit_certificates<T: CertDataVisitor>(cert_dir: &PathBuf, visitor: T) -> anyhow::Result<T> {
    let keystore = X509Keystore::load(cert_dir)?;
    let mut visitor = X509Visitor { visitor };
    keystore.visit_certs(&mut visitor)?;
    Ok(visitor.visitor)
}

pub(crate) struct X509Visitor<T: CertDataVisitor> {
    visitor: T,
}

impl<T: CertDataVisitor> X509Visitor<T> {
    pub(crate) fn accept(
        &mut self,
        cert: &X509Ref,
        permissions: &PermissionsManager,
    ) -> anyhow::Result<()> {
        let cert_data = CertData::create(cert, permissions)?;
        self.visitor.accept(cert_data);
        Ok(())
    }
}

impl TryFrom<&X509Ref> for CertData {
    type Error = anyhow::Error;

    fn try_from(cert: &X509Ref) -> Result<Self, Self::Error> {
        let id = cert_to_id(cert)?;
        let not_after = cert.not_after().to_string();
        let mut subject = BTreeMap::new();
        add_cert_subject_entries(&mut subject, cert, Nid::COMMONNAME, "CN");
        add_cert_subject_entries(&mut subject, cert, Nid::PKCS9_EMAILADDRESS, "E");
        add_cert_subject_entries(&mut subject, cert, Nid::ORGANIZATIONNAME, "O");
        add_cert_subject_entries(&mut subject, cert, Nid::ORGANIZATIONALUNITNAME, "OU");
        add_cert_subject_entries(&mut subject, cert, Nid::COUNTRYNAME, "C");
        add_cert_subject_entries(&mut subject, cert, Nid::STATEORPROVINCENAME, "ST");

        Ok(CertData {
            id,
            not_after,
            subject,
            permissions: "".to_string(),
        })
    }
}

/// Adds entries with given `nid` to given `subject` String.
fn add_cert_subject_entries(
    subject: &mut BTreeMap<String, String>,
    cert: &X509Ref,
    nid: Nid,
    entry_short_name: &str,
) {
    if let Some(entries) = cert_subject_entries(cert, nid) {
        subject.insert(entry_short_name.to_string(), entries);
    }
}

/// Reads subject entries and returns them as comma separated `String`.
fn cert_subject_entries(cert: &X509Ref, nid: Nid) -> Option<String> {
    let entries =
        cert.subject_name()
            .entries_by_nid(nid)
            .fold(String::from(""), |mut names, name| {
                if !names.is_empty() {
                    names.push_str(", ");
                }
                let name = String::from_utf8_lossy(name.data().as_slice());
                names.push_str(&name);
                names
            });
    if entries.is_empty() {
        return None;
    }
    Some(entries)
}

pub enum KeystoreRemoveResult {
    NothingToRemove,
    Removed { removed: Vec<X509> },
}

#[derive(Clone, Default)]
pub struct PermissionsManager {
    permissions: HashMap<String, Vec<CertPermissions>>,
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
        permissions: &Vec<CertPermissions>,
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

impl std::fmt::Debug for X509Keystore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Keystore")
    }
}
