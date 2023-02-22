pub mod golem_keystore;
pub mod x509_keystore;

use itertools::Itertools;

use crate::policy::CertPermissions;

use self::{
    golem_keystore::GolemCertAddParams,
    x509_keystore::{KeystoreRemoveResult, PermissionsManager, X509AddParams, X509KeystoreManager},
};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};

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

pub struct CertData {
    pub id: String,
    pub not_after: String,
    pub subject: BTreeMap<String, String>,
    pub permissions: String,
}

#[derive(Default)]
pub struct AddResponse {
    added: Vec<CertData>,
    skipped: Vec<CertData>,
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
struct RemoveResponse {
    removed: Vec<CertData>,
    skipped: Vec<CertData>,
}

trait Keystore {
    fn add(&mut self, add: &AddParams) -> anyhow::Result<AddResponse>;
    fn remove(&mut self, remove: &RemoveParams) -> anyhow::Result<RemoveResponse>;
}

pub struct CompositeKeystore {
    keystores: Vec<Box<dyn Keystore>>,
}

impl CompositeKeystore {
    pub fn try_new(cert_dir: &PathBuf) -> anyhow::Result<Self> {
        let x509_keystore_manager = X509KeystoreManager::try_load(cert_dir)?;
        let keystores: Vec<Box<dyn Keystore>> = vec![Box::new(x509_keystore_manager)];
        Ok(Self { keystores })
    }

    pub fn add(&mut self, add: AddParams) -> anyhow::Result<AddResponse> {
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

    pub fn remove(&mut self) {
        todo!()
    }

    fn list(&self) {
        todo!()
    }
}
