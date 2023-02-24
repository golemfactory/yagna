pub mod golem_keystore;
pub mod x509_keystore;

use self::x509_keystore::{X509AddParams, X509KeystoreManager, X509CertData};
use crate::policy::CertPermissions;
use itertools::Itertools;
use std::{
    collections::{BTreeMap, HashSet},
    path::{PathBuf, Path}, fmt::Debug,
};

pub enum Cert {
    X509(X509CertData),
    Golem{ id: String, cert: super::golem_certificate::GolemCertificate }
}

impl Cert {
    pub fn id(&self) -> String {
        match self {
            Cert::Golem { id, cert: _ } => id.into(),
            Cert::X509(cert) => cert.id.to_string()
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

impl X509AddParams for AddParams {
    fn permissions(&self) -> &Vec<crate::policy::CertPermissions> {
        &self.permissions
    }

    fn whole_chain(&self) -> bool {
        self.whole_chain
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

// trait Keystore: Debug {
pub trait Keystore {
    fn load(cert_dir: &PathBuf) -> anyhow::Result<Self> where Self: Sized;
    fn reload(&mut self, cert_dir: &PathBuf) -> anyhow::Result<()>;
    fn add(&mut self, add: &AddParams) -> anyhow::Result<AddResponse>;
    fn remove(&mut self, remove: &RemoveParams) -> anyhow::Result<RemoveResponse>;
    fn list(&self) -> Vec<Cert>;
}

// #[derive(Debug)]
pub struct CompositeKeystore {
    keystores: Vec<Box<dyn Keystore>>,
}

impl Keystore for CompositeKeystore {
    fn load(cert_dir: &PathBuf) -> anyhow::Result<Self> {
        let x509_keystore_manager = X509KeystoreManager::load(cert_dir)?;
        let keystores: Vec<Box<dyn Keystore>> = vec![Box::new(x509_keystore_manager)];
        Ok(Self { keystores })
    }

    fn reload(&mut self, cert_dir: &PathBuf) -> anyhow::Result<()> {
        for keystore in &mut self.keystores {
            keystore.reload(cert_dir)?;
        }
        Ok(())
    }

    fn add(&mut self, add: &AddParams) -> anyhow::Result<AddResponse> {
        let response = self
            .keystores
            .iter_mut()
            .map(|keystore| keystore.add(&add))
            .fold_ok(AddResponse::default(), |mut acc, mut res| {
                acc.added.append(&mut res.added);
                acc.skipped.append(&mut res.skipped);
                acc
            })?;
        Ok(response)
    }

    fn remove(&mut self, remove: &RemoveParams) -> anyhow::Result<RemoveResponse> {
        let response = self
            .keystores
            .iter_mut()
            .map(|keystore| keystore.remove(&remove))
            .fold_ok(RemoveResponse::default(), |mut acc, mut res| {
                acc.removed.append(&mut res.removed);
                acc
            })?;
        Ok(response)
    }

    fn list(&self) -> Vec<Cert> {
        self.keystores
            .iter()
            .map(|keystore| keystore.list())
            .flatten()
            .collect()
    }
}
