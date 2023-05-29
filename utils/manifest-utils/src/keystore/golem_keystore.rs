use super::{Cert, Keystore, KeystoreBuilder};
use crate::keystore::copy_file;
use anyhow::anyhow;
use chrono::{DateTime, Utc};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use golem_certificate::validator::validated_data::ValidatedNodeDescriptor;
use golem_certificate::validator::validated_data::ValidatedCertificate;

pub const CERT_NAME: &str = "Golem";

#[derive(Debug, Clone)]
pub struct GolemCertificateEntry {
    #[allow(dead_code)]
    path: PathBuf,
    cert: ValidatedCertificate,
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
        let (id, cert) = read_cert(cert_path, None)?;
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

/// Returns validated certificate with its id.
/// # Arguments
/// * `cert_path` path to Golem certificate file
/// * `timestamp` optional timestamp to verify validity
fn read_cert(cert_path: &Path, timestamp: Option<DateTime<Utc>>) -> anyhow::Result<(String, ValidatedCertificate)> {
    let mut cert_file = File::open(cert_path)?;
    let mut cert_content = String::new();
    cert_file.read_to_string(&mut cert_content)?;
    let cert_content = cert_content.trim();
    let cert = golem_certificate::validator::validate_certificate_str(
        cert_content,
        timestamp,
    )?;
    let id = cert
        .certificate_chain_fingerprints
        .get(0)
        .ok_or_else(|| anyhow!("No leaf cert id found in {CERT_NAME} certificate"))?
        .to_owned();

    Ok((id, cert))
}

#[derive(Debug, Clone)]
pub(super) struct GolemKeystore {
    pub certificates: Arc<RwLock<HashMap<String, GolemCertificateEntry>>>,
    pub cert_dir: PathBuf,
}

impl GolemKeystore {
    pub fn verify_node_descriptor(
        &self,
        node_descriptor: serde_json::Value,
    ) -> anyhow::Result<ValidatedNodeDescriptor> {
        golem_certificate::validator::validate_node_descriptor(node_descriptor)
            .map_err(|e| anyhow!("verification of node descriptor failed: {e}"))
    }
}

impl Keystore for GolemKeystore {
    fn reload(&self, cert_dir: &Path) -> anyhow::Result<()> {
        let mut certificates = HashMap::new();
        let cert_dir = std::fs::read_dir(cert_dir)?;
        for dir_entry in cert_dir {
            let file = dir_entry?;
            let path = file.path();
            match read_cert(&path, None) {
                Ok((id, cert)) => {
                    let cert = GolemCertificateEntry { path, cert };
                    certificates.insert(id, cert);
                }
                Err(err) => {
                    log::trace!("Unable to parse file '{path:?}' as {CERT_NAME} cert. Err: {err}")
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
        let mut invalid = Vec::new();
        let mut certificates = self
            .certificates
            .write()
            .expect("Can't read Golem keystore");
        let mut leaf_cert_ids = Vec::new();
        for path in add.certs.iter() {
            match read_cert(path.as_path(), None) {
                Ok((id, cert)) => {
                    if certificates.contains_key(&id) {
                        skipped.push(Cert::Golem { id, cert });
                        continue;
                    } else if cert.validity_period.not_after < chrono::Utc::now() {
                        log::error!("Expired Golem certificate {:?}.", cert.validity_period);
                        invalid.push(path.clone());
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
                    leaf_cert_ids.push(id.clone());
                    added.push(Cert::Golem { id, cert })
                }
                Err(err) => {
                    log::error!("Unable to parse Golem certificate. Err: {}", err);
                    invalid.push(path.clone());
                }
            }
        }
        Ok(super::AddResponse {
            added,
            duplicated: skipped,
            invalid,
            leaf_cert_ids,
        })
    }

    fn remove(&mut self, remove: &super::RemoveParams) -> anyhow::Result<super::RemoveResponse> {
        let mut certificates = self
            .certificates
            .write()
            .expect("Can't write Golem keystore");
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
            .expect("Can't read Golem keystore")
            .iter()
        {
            certificates.push(Cert::Golem {
                id: id.clone(),
                cert: cert_entry.cert.clone(),
            });
        }
        certificates
    }

    fn verifier(&self, _: &str) -> anyhow::Result<Box<dyn super::SignatureVerifier>> {
        anyhow::bail!("NYI")
    }
}
