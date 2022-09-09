use std::fs::{self, File};
use std::str;
use std::sync::Once;
use std::{collections::HashSet, path::PathBuf};

use openssl::hash::MessageDigest;
use openssl::pkey::PKey;
use openssl::rsa::Rsa;
use openssl::sign::Signer;
use tar::Archive;

use ya_manifest_utils::{
    util::{self, CertBasicData, CertBasicDataVisitor},
    KeystoreLoadResult, KeystoreRemoveResult,
};

static INIT: Once = Once::new();

#[allow(clippy::ptr_arg)]
pub fn load_certificates_from_dir(
    resource_cert_dir: &PathBuf,
    test_cert_dir: &PathBuf,
    certfile_names: &[&str],
) -> KeystoreLoadResult {
    let cert_paths: Vec<PathBuf> = certfile_names
        .iter()
        .map(|certfile_name| {
            let mut cert_path = resource_cert_dir.clone();
            cert_path.push(certfile_name);
            cert_path
        })
        .collect();
    let keystore_manager =
        util::KeystoreManager::try_new(test_cert_dir).expect("Can create keystore manager");
    keystore_manager
        .load_certs(&cert_paths)
        .expect("Can load certificates")
}

pub fn remove_certificates(test_cert_dir: &PathBuf, cert_ids: &[&str]) -> KeystoreRemoveResult {
    let keystore_manager =
        util::KeystoreManager::try_new(test_cert_dir).expect("Can create keystore manager");
    keystore_manager
        .remove_certs(&slice_to_set(cert_ids))
        .expect("Can load certificates")
}

#[derive(Default)]
pub struct TestCertDataVisitor {
    expected: HashSet<String>,
    actual: HashSet<String>,
}

impl TestCertDataVisitor {
    #[allow(dead_code)]
    pub fn new(expected: &[&str]) -> Self {
        Self {
            expected: expected.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[allow(dead_code)]
    pub fn test(&self) {
        assert_eq!(self.expected, self.actual)
    }
}

impl CertBasicDataVisitor for TestCertDataVisitor {
    fn accept(&mut self, cert_data: CertBasicData) {
        self.actual.insert(cert_data.id);
    }
}

pub struct TestResources {
    pub temp_dir: &'static str,
}

impl TestResources {
    /// Creates new empty cert directory and resources directory with unpacked certificates.
    pub fn init_cert_dirs(&self) -> (PathBuf, PathBuf) {
        let resource_cert_dir = self.resource_cert_dir_path();
        INIT.call_once(|| {
            if resource_cert_dir.exists() {
                fs::remove_dir_all(&resource_cert_dir).expect("Can delete test cert resources dir");
            }
            fs::create_dir_all(&resource_cert_dir).expect("Can create temp dir");
            self.unpack_cert_resources(&resource_cert_dir);
        });
        let store_cert_dir = self.store_cert_dir_path();
        if store_cert_dir.exists() {
            // we want to clean store cert dir before every test
            fs::remove_dir_all(&store_cert_dir).expect("Can delete test temp dir");
        }
        fs::create_dir_all(&store_cert_dir).expect("Can create temp dir");
        (resource_cert_dir, store_cert_dir)
    }

    pub fn loaded_cert_files(&self) -> HashSet<String> {
        let store_cert_dir = self.store_cert_dir_path();
        let cert_dir = fs::read_dir(store_cert_dir).expect("Can read cert dir");
        cert_dir
            .into_iter()
            .map(|file| file.expect("Can list cert files"))
            .map(|x| x.file_name().to_string_lossy().to_string())
            .collect()
    }

    // Signs given `data_b64` using `signing_key` (filename) and returns base64 encoded signature.
    pub fn sign_data(&self, data_b64: &[u8], signing_key: &str) -> String {
        let mut signing_key_path = self.resource_cert_dir_path();
        signing_key_path.push(signing_key);
        let signing_key = fs::read(signing_key_path).expect("Can read signing key");
        let mut password = self.resource_cert_dir_path();
        password.push("pass.txt");
        let password = fs::read(password).expect("Can read password file");
        let password = str::from_utf8(&password).unwrap().trim(); // just in case it got newline at the end
        let keypair = Rsa::private_key_from_pem_passphrase(&signing_key, password.as_bytes())
            .expect("Can parse signing key");
        let keypair = PKey::from_rsa(keypair).unwrap();
        let mut signer = Signer::new(MessageDigest::sha256(), &keypair).unwrap();
        signer.update(data_b64).unwrap();
        let signature = signer.sign_to_vec().expect("Can sign manifest");
        base64::encode(signature)
    }

    fn unpack_cert_resources(&self, cert_resources_dir: &PathBuf) {
        let mut cert_archive = self.test_resources_dir_path();
        cert_archive.push("certificates.tar");
        let cert_archive = File::open(cert_archive).expect("Can open cert archive file");
        let mut cert_archive = Archive::new(cert_archive);
        cert_archive
            .unpack(cert_resources_dir)
            .expect("Can unack cert archive");
    }

    pub fn test_resources_dir_path(&self) -> PathBuf {
        let mut test_resources = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        test_resources.push("resources/test");
        test_resources
    }

    fn resource_cert_dir_path(&self) -> PathBuf {
        let mut cert_resources = self.temp_dir_path();
        cert_resources.push("cert_resources");
        cert_resources
    }

    fn store_cert_dir_path(&self) -> PathBuf {
        let mut cert_dir = self.temp_dir_path();
        cert_dir.push("cert_dir");
        cert_dir
    }

    fn temp_dir_path(&self) -> PathBuf {
        PathBuf::from(self.temp_dir)
    }
}

pub fn slice_to_set<T: AsRef<str>>(v: &[T]) -> HashSet<String> {
    v.iter()
        .map(|s| s.as_ref().to_string())
        .collect::<HashSet<String>>()
}
