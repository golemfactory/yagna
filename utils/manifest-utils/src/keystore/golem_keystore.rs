use super::{Cert, Keystore, KeystoreBuilder};
use crate::{
    golem_certificate::{self, GolemCertificate},
    keystore::copy_file,
    util::str_to_short_hash,
};
use anyhow::anyhow;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

#[derive(Debug, Clone)]
pub struct GolemCertificateEntry {
    #[allow(dead_code)]
    path: PathBuf,
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
    fn try_with(&mut self, cert_path: &Path) -> anyhow::Result<()> {
        let (id, cert) = read_cert(cert_path)?;
        let file = PathBuf::from(cert_path);
        self.certificates
            .insert(id, GolemCertificateEntry { path: file, cert });
        Ok(())
    }

    fn build(self) -> anyhow::Result<GolemKeystore> {
        let certificates = Arc::new(RwLock::new(self.certificates));
        let cert_dir = self.cert_dir;
        Ok(GolemKeystore {
            certificates,
            cert_dir,
        })
    }
}

// Return certificate with its id
fn read_cert(cert_path: &Path) -> anyhow::Result<(String, GolemCertificate)> {
    let mut cert_file = File::open(cert_path)?;
    let mut cert_content = String::new();
    cert_file.read_to_string(&mut cert_content)?;
    let cert_content = cert_content.trim();
    let cert = golem_certificate::verify_golem_certificate(cert_content)?;
    let id = str_to_short_hash(cert_content);
    Ok((id, cert))
}

#[derive(Debug, Clone)]
pub(super) struct GolemKeystore {
    pub certificates: Arc<RwLock<HashMap<String, GolemCertificateEntry>>>,
    pub cert_dir: PathBuf,
}

impl GolemKeystore {
    pub fn verify_golem_certificate(&self, cert: &str) -> anyhow::Result<GolemCertificate> {
        golem_certificate::verify_golem_certificate(cert)
            .map_err(|e| anyhow!("verification of golem certificate failed: {e}"))
    }
}

impl Keystore for GolemKeystore {
    fn reload(&self, cert_dir: &Path) -> anyhow::Result<()> {
        let mut certificates = HashMap::new();
        let cert_dir = std::fs::read_dir(cert_dir)?;
        for dir_entry in cert_dir {
            let file = dir_entry?;
            let path = file.path();
            match read_cert(&path) {
                Ok((id, cert)) => {
                    let cert = GolemCertificateEntry { path, cert };
                    certificates.insert(id, cert);
                }
                Err(err) => {
                    log::trace!("Unable to parse file '{path:?}' as Golem cert. Err: {err}")
                }
            }
        }
        let mut certificates_ref = self.certificates.write().unwrap();
        *certificates_ref = certificates;
        Ok(())
    }

    fn add(&mut self, add: &super::AddParams) -> anyhow::Result<super::AddResponse> {
        let mut added = Vec::new();
        let mut skipped = Vec::new();
        let mut certificates = self.certificates.write().expect("Can read Golem keystore");
        for path in add.certs.iter() {
            let mut file = File::open(path)?;
            let mut content = String::new();
            file.read_to_string(&mut content)?;
            let content = content.trim().to_string();
            let id = str_to_short_hash(&content);
            match self.verify_golem_certificate(&content) {
                Ok(cert) => {
                    if certificates.contains_key(&id) {
                        skipped.push(Cert::Golem { id, cert });
                        continue;
                    }
                    log::debug!("Adding Golem certificate: {:?}", cert);
                    let path = copy_file(path, &self.cert_dir)?;
                    certificates.insert(
                        id.clone(),
                        GolemCertificateEntry {
                            path,
                            cert: cert.clone(),
                        },
                    );
                    added.push(Cert::Golem { id, cert })
                }
                Err(err) => log::warn!("Unable to parse Golem certificate. Err: {}", err),
            }
        }
        Ok(super::AddResponse { added, skipped })
    }

    fn remove(&mut self, remove: &super::RemoveParams) -> anyhow::Result<super::RemoveResponse> {
        let mut certificates = self.certificates.write().expect("Can write Golem keystore");
        let mut removed = Vec::new();
        for id in &remove.ids {
            if let Some(GolemCertificateEntry { path, cert }) = certificates.remove(id) {
                log::debug!("Removing Golem certificate: {:?}", cert);
                fs::remove_file(path)?;
                let id = id.clone();
                removed.push(Cert::Golem { id, cert });
            }
        }
        Ok(super::RemoveResponse { removed })
    }

    fn list(&self) -> Vec<super::Cert> {
        let mut certificates = Vec::new();
        for (id, cert_entry) in self
            .certificates
            .read()
            .expect("Can read Golem keystore")
            .iter()
        {
            certificates.push(Cert::Golem {
                id: id.clone(),
                cert: cert_entry.cert.clone(),
            });
        }
        certificates
    }
}
