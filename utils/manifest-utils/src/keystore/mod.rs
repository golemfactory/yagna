pub mod x509;

use self::x509::{
    KeystoreLoadResult, KeystoreRemoveResult, PermissionsManager, X509KeystoreManager,
};
use std::{collections::HashSet, path::PathBuf};

trait Keystore {
    fn load();
    fn delete(&mut self);
    fn list(&self);
}

struct CompositeKeystore {}

impl Keystore for CompositeKeystore {
    fn load() {
        todo!()
    }

    fn delete(&mut self) {
        todo!()
    }

    fn list(&self) {
        todo!()
    }
}

pub struct KeystoreManager {
    x509_keystore_manager: X509KeystoreManager,
}

impl KeystoreManager {
    pub fn try_new(cert_dir: &PathBuf) -> anyhow::Result<Self> {
        let x509_keystore_manager = X509KeystoreManager::try_new(cert_dir)?;
        Ok(Self {
            x509_keystore_manager,
        })
    }

    /// Copies certificates from given file to `cert-dir` and returns newly added certificates.
    /// Certificates already existing in `cert-dir` are skipped.
    pub fn load_certs(self, cert_paths: &Vec<PathBuf>) -> anyhow::Result<KeystoreLoadResult> {
        self.x509_keystore_manager.load_certs(cert_paths)
    }

    pub fn remove_certs(self, ids: &HashSet<String>) -> anyhow::Result<KeystoreRemoveResult> {
        self.x509_keystore_manager.remove_certs(ids)
    }

    pub fn permissions_manager(&self) -> PermissionsManager {
        self.x509_keystore_manager.permissions_manager()
    }
}
