pub mod golem_keystore;
pub mod x509_keystore;

use self::{
    golem_keystore::GolemKeystoreBuilder,
    x509_keystore::{X509AddParams, X509CertData, X509KeystoreBuilder, X509KeystoreManager},
};
use crate::{
    golem_certificate::{GolemCertificate, VerificationError},
    policy::CertPermissions,
};
use itertools::Itertools;
use std::{
    borrow::{Borrow, BorrowMut},
    cell::RefCell,
    collections::HashSet,
    fs::File,
    io::Read,
    path::PathBuf,
    rc::Rc,
};

pub enum Cert {
    X509(X509CertData),
    Golem {
        id: String,
        cert: super::golem_certificate::GolemCertificate,
    },
}

impl Cert {
    pub fn id(&self) -> String {
        match self {
            Cert::Golem { id, cert: _ } => id.into(),
            Cert::X509(cert) => cert.id.to_string(),
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

#[derive(Clone)]
pub struct CompositeKeystore {
    keystores: Rc<RefCell<Vec<Box<dyn Keystore>>>>,
}

impl CompositeKeystore {
    pub fn load(cert_dir: &PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&cert_dir)?;
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

        let keystores: Rc<RefCell<Vec<Box<dyn Keystore>>>> = Rc::new(RefCell::new(vec![
            Box::new(golem_keystore),
            Box::new(x509_keystore),
        ]));
        Ok(Self { keystores })
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
        // verify cert, then
        todo!()
    }

    pub fn verify_golem_certificate(
        &self,
        node_id: &String,
    ) -> Result<GolemCertificate, VerificationError> {
        todo!()
    }
}

impl Keystore for CompositeKeystore {
    fn reload(&self, cert_dir: &PathBuf) -> anyhow::Result<()> {
        for keystore in (&*self.keystores).borrow().iter() {
            keystore.reload(cert_dir)?;
        }
        Ok(())
    }

    fn add(&mut self, add: &AddParams) -> anyhow::Result<AddResponse> {
        let response = (&*self.keystores)
            .borrow_mut()
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
        let response = (&*self.keystores)
            .borrow_mut()
            .iter_mut()
            .map(|keystore| keystore.remove(&remove))
            .fold_ok(RemoveResponse::default(), |mut acc, mut res| {
                acc.removed.append(&mut res.removed);
                acc
            })?;
        Ok(response)
    }

    fn list(&self) -> Vec<Cert> {
        (&*self.keystores)
            .borrow()
            .iter()
            .map(|keystore| keystore.list())
            .flatten()
            .collect()
    }
}
