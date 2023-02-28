pub mod golem_keystore;
pub mod x509_keystore;

use self::{
    golem_keystore::{GolemKeystore, GolemKeystoreBuilder},
    x509_keystore::{X509CertData, X509KeystoreBuilder, X509KeystoreManager},
};
use crate::policy::CertPermissions;
use golem_certificate::{
    schemas::certificate::Certificate, validator::validated_data::ValidatedNodeDescriptor,
};
use itertools::Itertools;
use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

pub enum Cert {
    X509(X509CertData),
    Golem { id: String, cert: Certificate },
}

impl Cert {
    pub fn id(&self) -> String {
        match self {
            Cert::Golem { id, cert: _ } => id.into(),
            Cert::X509(cert) => cert.id.to_string(),
        }
    }

    pub fn not_after(&self) -> String {
        match self {
            Cert::X509(cert) => cert.not_after.clone(),
            Cert::Golem { id: _, cert: _ } => "".into(),
        }
    }

    pub fn subject(&self) -> String {
        match self {
            Cert::X509(cert) => {
                serde_json::to_string(&cert.subject).expect("Can serialize X509 fields")
            }
            Cert::Golem { id: _, cert: _ } => "".into(),
        }
    }
}

trait CommonAddParams {
    fn certs(&self) -> &Vec<PathBuf>;
}

pub struct AddParams {
    pub permissions: Vec<CertPermissions>,
    pub certs: Vec<PathBuf>,
    pub whole_chain: bool,
}

impl AddParams {
    pub fn new(certs: Vec<PathBuf>) -> Self {
        let permissions = vec![CertPermissions::All];
        let whole_chain = true;
        Self {
            permissions,
            certs,
            whole_chain,
        }
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
    pub skipped: Vec<Cert>,
}

pub trait CommonRemoveParams {
    fn id(&self) -> &HashSet<String>;
}

pub struct RemoveParams {
    pub ids: HashSet<String>,
}

impl CommonRemoveParams for RemoveParams {
    fn id(&self) -> &HashSet<String> {
        &self.ids
    }
}

#[derive(Default)]
pub struct RemoveResponse {
    pub removed: Vec<Cert>,
}

pub trait Keystore: KeystoreClone + Send {
    fn reload(&self, cert_dir: &PathBuf) -> anyhow::Result<()>;
    fn add(&mut self, add: &AddParams) -> anyhow::Result<AddResponse>;
    fn remove(&mut self, remove: &RemoveParams) -> anyhow::Result<RemoveResponse>;
    fn list(&self) -> Vec<Cert>;
}

trait KeystoreBuilder<K: Keystore> {
    // file is certificate file, certificate is its content
    fn try_with(&mut self, file: &PathBuf) -> anyhow::Result<()>;
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

    fn keystores(&self) -> Vec<Box<&dyn Keystore>> {
        vec![
            Box::new(&self.golem_keystore),
            Box::new(&self.x509_keystore),
        ]
    }

    fn keystores_mut(&mut self) -> Vec<Box<&mut dyn Keystore>> {
        vec![
            Box::new(&mut self.golem_keystore),
            Box::new(&mut self.x509_keystore),
        ]
    }

    pub fn list_ids(&self) -> HashSet<String> {
        self.list()
            .into_iter()
            .map(|cert| cert.id())
            .collect::<HashSet<String>>()
    }

    pub fn verify_signature(
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

    pub fn verify_node_descriptor(&self, cert: &String) -> anyhow::Result<ValidatedNodeDescriptor> {
        self.golem_keystore.verify_node_descriptor(cert)
    }
}

impl Keystore for CompositeKeystore {
    fn reload(&self, cert_dir: &PathBuf) -> anyhow::Result<()> {
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
                acc.skipped.append(&mut res.skipped);
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
        self.keystores()
            .iter()
            .flat_map(|keystore| keystore.list())
            .collect()
    }
}

/// Copies file into `dst_dir`.
/// Renames file if duplicated: "name.ext" into "name.1.ext" etc.
fn copy_file(src_file: &PathBuf, dst_dir: &PathBuf) -> anyhow::Result<()> {
    let file_name = get_file_name(src_file)
        .ok_or_else(|| anyhow::anyhow!(format!("Cannot get filename of {src_file:?}")))?;
    let mut new_cert_path = dst_dir.clone();
    new_cert_path.push(file_name);
    if new_cert_path.exists() {
        let file_stem = get_file_stem(&new_cert_path).expect("Has to have stem");
        let dot_extension = get_file_extension(&new_cert_path)
            .map(|ex| format!(".{ex}"))
            .unwrap_or_else(|| String::from(""));
        for i in 0..u32::MAX {
            let numbered_filename = format!("{file_stem}.{i}{dot_extension}");
            new_cert_path = dst_dir.clone();
            new_cert_path.push(numbered_filename);
            if !new_cert_path.exists() {
                break;
            }
        }
        if new_cert_path.exists() {
            anyhow::bail!("Unable to load certificate");
        }
    }
    fs::copy(src_file, new_cert_path)?;
    Ok(())
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
