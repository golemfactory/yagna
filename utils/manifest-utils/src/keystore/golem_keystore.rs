use super::{Cert, Keystore, KeystoreBuilder};
use crate::keystore::copy_file;
use anyhow::anyhow;
use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use golem_certificate::validator::validated_data::ValidatedCert;
use golem_certificate::{
    schemas::certificate::Certificate, validator::validated_data::ValidatedNodeDescriptor,
};

#[derive(Debug, Clone)]
pub struct GolemCertificateEntry {
    #[allow(dead_code)]
    path: PathBuf,
    cert: Certificate,
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
    fn try_with(&mut self, cert_path: &PathBuf) -> anyhow::Result<()> {
        let (id, cert) = read_cert(cert_path)?;
        let file = cert_path.clone();
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
fn read_cert(cert_path: &PathBuf) -> anyhow::Result<(String, Certificate)> {
    let mut cert_file = File::open(cert_path)?;
    let mut cert_content = String::new();
    cert_file.read_to_string(&mut cert_content)?;
    let cert_content = cert_content.trim();
    let cert = golem_certificate::validator::validate_golem_certificate(cert_content)?;
    Ok((cert.chain[0].clone(), cert.cert))
}

#[derive(Debug, Clone)]
pub(super) struct GolemKeystore {
    pub certificates: Arc<RwLock<HashMap<String, GolemCertificateEntry>>>,
    pub cert_dir: PathBuf,
}

impl GolemKeystore {
    pub fn verify_node_descriptor(&self, cert: &str) -> anyhow::Result<ValidatedNodeDescriptor> {
        golem_certificate::validator::validate_node_descriptor(cert)
            .map_err(|e| anyhow!("verification of golem certificate failed: {e}"))
    }

    pub fn verify_golem_certificate(&self, cert: &str) -> anyhow::Result<ValidatedCert> {
        golem_certificate::validator::validate_golem_certificate(cert)
            .map_err(|e| anyhow!("verification of golem certificate failed: {e}"))
    }
}

impl Keystore for GolemKeystore {
    fn reload(&self, cert_dir: &PathBuf) -> anyhow::Result<()> {
        let mut certificates = HashMap::new();
        let cert_dir = std::fs::read_dir(cert_dir)?;
        for dir_entry in cert_dir {
            let file = dir_entry?;
            let cert_path = file.path();
            match read_cert(&cert_path) {
                Ok((id, cert)) => {
                    certificates.insert(id, cert);
                }
                Err(err) => log::trace!(
                    "Unable to parse file '{:?}' as Golem cert. Err: {}",
                    cert_path,
                    err
                ),
            }
        }
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
            match self.verify_golem_certificate(&content) {
                Ok(cert) => {
                    let id = cert.chain[0].clone();
                    if certificates.contains_key(&id) {
                        skipped.push(Cert::Golem {
                            id,
                            cert: cert.cert,
                        });
                        continue;
                    }
                    log::debug!("Adding Golem certificate: {:?}", cert);
                    copy_file(path, &self.cert_dir)?;
                    certificates.insert(
                        id.clone(),
                        GolemCertificateEntry {
                            path: path.clone(),
                            cert: cert.cert.clone(),
                        },
                    );
                    added.push(Cert::Golem {
                        id,
                        cert: cert.cert,
                    })
                }
                Err(err) => log::error!("Failed to parse Golem certificate. Err: {}", err),
            }
        }
        Ok(super::AddResponse { added, skipped })
    }

    fn remove(&mut self, _remove: &super::RemoveParams) -> anyhow::Result<super::RemoveResponse> {
        Ok(Default::default())
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
