pub mod golem_keystore;
pub mod x509_keystore;

use self::{
    golem_keystore::{GolemKeystore, GolemKeystoreBuilder},
    x509_keystore::{X509CertData, X509KeystoreBuilder, X509KeystoreManager},
};
use chrono::SecondsFormat;
use golem_certificate::validator::validated_data::{ValidatedCertificate, ValidatedNodeDescriptor};
use itertools::Itertools;
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashSet},
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

// Large enum variant caused by flatten maps of possible additional fields in 'ValidatedCertificate'.
#[allow(clippy::large_enum_variant)]
#[derive(Eq, PartialEq)]
pub enum Cert {
    X509(X509CertData),
    Golem {
        id: String,
        cert: ValidatedCertificate,
    },
}

impl Cert {
    /// Certificate id (long).
    pub fn id(&self) -> String {
        match self {
            Cert::Golem { id, cert: _ } => id.into(),
            Cert::X509(cert) => cert.id.to_string(),
        }
    }

    /// Not_after date in RFC3339 format.
    pub fn not_after(&self) -> String {
        let not_after = match self {
            Cert::X509(cert) => cert.not_after,
            Cert::Golem { id: _, cert } => cert.validity_period.not_after,
        };
        not_after.to_rfc3339_opts(SecondsFormat::Secs, true)
    }

    /// Subject displayed Json value.
    /// Json for X.509 certificate, 'display_name' for Golem certificate.
    pub fn subject(&self) -> Value {
        match self {
            Cert::X509(cert) => serde_json::json!(cert.subject),
            Cert::Golem { cert, .. } => serde_json::json!(cert.subject.display_name),
        }
    }

    pub fn type_name(&self) -> &str {
        match self {
            Cert::X509(_) => x509_keystore::CERT_NAME,
            Cert::Golem { .. } => golem_keystore::CERT_NAME,
        }
    }
}

impl PartialOrd for Cert {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.id().partial_cmp(&other.id())
    }
}

impl Ord for Cert {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id().cmp(&other.id())
    }
}
trait CommonAddParams {
    fn certs(&self) -> &Vec<PathBuf>;
}

pub struct AddParams {
    pub certs: Vec<PathBuf>,
}

impl AddParams {
    pub fn new(certs: Vec<PathBuf>) -> Self {
        Self { certs }
    }
}

impl CommonAddParams for AddParams {
    fn certs(&self) -> &Vec<PathBuf> {
        &self.certs
    }
}

#[derive(Default)]
pub struct AddResponse {
    pub added: Vec<Cert>,
    pub duplicated: Vec<Cert>,
    pub invalid: Vec<PathBuf>,
    pub leaf_cert_ids: Vec<String>,
}

pub struct RemoveParams {
    pub ids: HashSet<String>,
}

#[derive(Default)]
pub struct RemoveResponse {
    pub removed: Vec<Cert>,
}

pub trait Keystore: KeystoreClone + Send {
    fn reload(&self, cert_dir: &Path) -> anyhow::Result<()>;
    fn add(&mut self, add: &AddParams) -> anyhow::Result<AddResponse>;
    fn remove(&mut self, remove: &RemoveParams) -> anyhow::Result<RemoveResponse>;
    fn list(&self) -> Vec<Cert>;
}

trait KeystoreBuilder<K: Keystore> {
    // file is certificate file, certificate is its content
    fn try_with(&mut self, file: &Path) -> anyhow::Result<()>;
    fn build(self) -> anyhow::Result<K>;
}

pub trait KeystoreClone {
    fn clone_box(&self) -> Box<dyn Keystore>;
}

impl<T> KeystoreClone for T
where
    T: 'static + Keystore + Clone,
{
    fn clone_box(&self) -> Box<dyn Keystore> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn Keystore> {
    fn clone(&self) -> Box<dyn Keystore> {
        self.clone_box()
    }
}

#[derive(Clone)]
pub struct CompositeKeystore {
    x509_keystore: X509KeystoreManager,
    golem_keystore: GolemKeystore,
}

impl CompositeKeystore {
    pub fn load(cert_dir: &PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(cert_dir)?;
        let mut x509_builder = X509KeystoreBuilder::new(cert_dir);
        let mut golem_builder = GolemKeystoreBuilder::new(cert_dir);

        let cert_dir = std::fs::read_dir(cert_dir)?;
        for dir_entry in cert_dir {
            let file = dir_entry?;
            let file = file.path();
            if let Err(err) = golem_builder
                .try_with(&file)
                .or_else(|err| {
                    log::trace!("File '{} is not a Golem certificate. Trying to parse as Golem certificate. Err: {err}'", file.display());
                    x509_builder.try_with(&file)
                }) {
                    log::debug!("File '{} is not a X509 certificate nor a Golem certificate. Err: {err}'", file.display());
                }
        }

        let x509_keystore = x509_builder.build()?;
        let golem_keystore = golem_builder.build()?;

        Ok(Self {
            x509_keystore,
            golem_keystore,
        })
    }

    fn keystores(&self) -> Vec<&dyn Keystore> {
        vec![&self.golem_keystore, &self.x509_keystore]
    }

    fn keystores_mut(&mut self) -> Vec<&mut dyn Keystore> {
        vec![&mut self.golem_keystore, &mut self.x509_keystore]
    }

    fn list_sorted(&self) -> BTreeSet<Cert> {
        self.keystores()
            .iter()
            .flat_map(|keystore| keystore.list())
            .collect::<BTreeSet<Cert>>()
    }

    pub fn list_ids(&self) -> Vec<String> {
        self.list_sorted()
            .into_iter()
            .map(|cert| cert.id())
            .collect::<Vec<String>>()
    }

    pub fn add_golem_cert(&mut self, add: &AddParams) -> anyhow::Result<AddResponse> {
        self.golem_keystore.add(add)
    }

    pub fn verify_x509_signature(
        &self,
        cert: impl AsRef<str>,
        sig: impl AsRef<str>,
        sig_alg: impl AsRef<str>,
        data: impl AsRef<str>,
    ) -> anyhow::Result<()> {
        self.x509_keystore
            .keystore
            .verify_signature(cert, sig, sig_alg, data)
    }

    pub fn verify_node_descriptor(
        &self,
        node_descriptor: serde_json::Value,
    ) -> anyhow::Result<ValidatedNodeDescriptor> {
        self.golem_keystore.verify_node_descriptor(node_descriptor)
    }
}

impl Keystore for CompositeKeystore {
    fn reload(&self, cert_dir: &Path) -> anyhow::Result<()> {
        for keystore in self.keystores().iter() {
            keystore.reload(cert_dir)?;
        }
        Ok(())
    }

    fn add(&mut self, add: &AddParams) -> anyhow::Result<AddResponse> {
        let response = self
            .keystores_mut()
            .iter_mut()
            .map(|keystore| keystore.add(add))
            .fold_ok(AddResponse::default(), |mut acc, mut res| {
                acc.added.append(&mut res.added);
                acc.duplicated.append(&mut res.duplicated);
                acc.invalid.append(&mut res.invalid);
                acc.leaf_cert_ids.append(&mut res.leaf_cert_ids);
                acc
            })?;
        Ok(response)
    }

    fn remove(&mut self, remove: &RemoveParams) -> anyhow::Result<RemoveResponse> {
        let response = self
            .keystores_mut()
            .iter_mut()
            .map(|keystore| keystore.remove(remove))
            .fold_ok(RemoveResponse::default(), |mut acc, mut res| {
                acc.removed.append(&mut res.removed);
                acc
            })?;
        Ok(response)
    }

    fn list(&self) -> Vec<Cert> {
        self.list_sorted().into_iter().collect()
    }
}

/// Copies file into `dst_dir`.
/// Renames file if duplicated: "name.ext" into "name.1.ext" etc.
fn copy_file(src_file: &PathBuf, dst_dir: &Path) -> anyhow::Result<PathBuf> {
    let file_name = get_file_name(src_file)
        .ok_or_else(|| anyhow::anyhow!(format!("Cannot get filename of {src_file:?}")))?;
    let mut new_cert_path = PathBuf::from(dst_dir);
    new_cert_path.push(file_name);
    if new_cert_path.exists() {
        let file_stem = get_file_stem(&new_cert_path).expect("Has to have stem");
        let dot_extension = get_file_extension(&new_cert_path)
            .map(|ex| format!(".{ex}"))
            .unwrap_or_else(|| String::from(""));
        for i in 0..u32::MAX {
            let numbered_filename = format!("{file_stem}.{i}{dot_extension}");
            new_cert_path = PathBuf::from(dst_dir);
            new_cert_path.push(numbered_filename);
            if !new_cert_path.exists() {
                break;
            }
        }
        if new_cert_path.exists() {
            anyhow::bail!("Unable to load certificate");
        }
    }
    fs::copy(src_file, &new_cert_path)?;
    Ok(new_cert_path)
}

fn get_file_name(path: &Path) -> Option<String> {
    path.file_name().map(os_str_to_string)
}

fn os_str_to_string(os_str: &OsStr) -> String {
    os_str.to_string_lossy().to_string()
}

fn get_file_extension(path: &Path) -> Option<String> {
    path.extension().map(os_str_to_string)
}

fn get_file_stem(path: &Path) -> Option<String> {
    path.file_stem().map(os_str_to_string)
}
