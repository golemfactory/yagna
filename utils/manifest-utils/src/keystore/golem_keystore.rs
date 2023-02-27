use super::{Cert, Keystore, KeystoreBuilder};
use crate::golem_certificate::{self, GolemCertificate};
use anyhow::anyhow;
use md5::{Digest, Md5};
use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct GolemCertificateEntry {
    file: PathBuf,
    cert: GolemCertificate,
}

pub(super) trait GolemCertAddParams {}

pub struct GolemKeystoreBuilder {
    pub certificates: HashMap<String, GolemCertificateEntry>,
    pub cert_dir: PathBuf,
}

impl GolemKeystoreBuilder {
    pub fn new(cert_dir: impl AsRef<Path>) -> Self {
        let certificates = Default::default();
        let cert_dir = PathBuf::from(cert_dir.as_ref());
        Self {
            certificates,
            cert_dir,
        }
    }
}

impl KeystoreBuilder<GolemKeystore> for GolemKeystoreBuilder {
    fn try_with(&mut self, cert_file: &PathBuf) -> anyhow::Result<()> {
        let mut cert = File::open(cert_file)?;
        let mut buffer = String::new();
        cert.read_to_string(&mut buffer)?;
        let cert = golem_certificate::verify_golem_certificate(&buffer)?;
        let id = Md5::digest(&buffer);
        let id = format!("{id:x}");
        self.certificates.insert(
            id,
            GolemCertificateEntry {
                file: cert_file.clone(),
                cert,
            },
        );
        Ok(())
    }

    fn build(self) -> anyhow::Result<GolemKeystore> {
        let certificates = self.certificates;
        let cert_dir = self.cert_dir;
        Ok(GolemKeystore {
            certificates,
            cert_dir,
        })
    }
}

#[derive(Debug, Clone)]
pub(super) struct GolemKeystore {
    pub certificates: HashMap<String, GolemCertificateEntry>,
    pub cert_dir: PathBuf,
}

impl GolemKeystore {
    pub fn verify_golem_certificate(&self, cert: &str) -> anyhow::Result<GolemCertificate> {
        golem_certificate::verify_golem_certificate(cert)
            .map_err(|e| anyhow!("verification of golem certificate failed: {e}"))
    }
}

impl Keystore for GolemKeystore {
    fn reload(&self, _cert_dir: &PathBuf) -> anyhow::Result<()> {
        todo!()
    }

    fn add(&mut self, _add: &super::AddParams) -> anyhow::Result<super::AddResponse> {
        Ok(Default::default())
    }

    fn remove(&mut self, _remove: &super::RemoveParams) -> anyhow::Result<super::RemoveResponse> {
        Ok(Default::default())
    }

    fn list(&self) -> Vec<super::Cert> {
        let mut certificates = Vec::new();
        for (id, cert_entry) in &self.certificates {
            certificates.push(Cert::Golem {
                id: id.clone(),
                cert: cert_entry.cert.clone(),
            });
        }
        certificates
    }
}
