use std::fs::{self, File};
use std::sync::Once;
use std::{collections::HashSet, path::PathBuf};

use tar::Archive;

use ya_manifest_utils::{
    util::{self, CertBasicData, CertBasicDataVisitor},
    KeystoreLoadResult, KeystoreRemoveResult,
};

static INIT: Once = Once::new();

pub fn load_certificates(
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
        .remove_certs(&to_set(cert_ids))
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
        let mut visitor = Self::default();
        visitor.expected = expected.iter().map(|s| s.to_string()).collect();
        visitor
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

pub fn init_cert_dirs() -> (PathBuf, PathBuf) {
    let resource_cert_dir = resource_cert_dir_path();
    INIT.call_once(|| {
        if resource_cert_dir.exists() {
            fs::remove_dir_all(&resource_cert_dir).expect("Can delete test cert resources dir");
        }
        fs::create_dir_all(&resource_cert_dir).expect("Can create temp dir");
        unpack_cert_resources(&resource_cert_dir);
    });
    let store_cert_dir = store_cert_dir_path();
    if store_cert_dir.exists() {
        // we want to clean store cert dir before every test
        fs::remove_dir_all(&store_cert_dir).expect("Can delete test temp dir");
    }
    fs::create_dir_all(&store_cert_dir).expect("Can create temp dir");
    (resource_cert_dir, store_cert_dir)
}

pub fn loaded_cert_files() -> HashSet<String> {
    let store_cert_dir = store_cert_dir_path();
    let cert_dir = fs::read_dir(store_cert_dir).expect("Can read cert dir");
    cert_dir
        .into_iter()
        .map(|file| file.expect("Can list cert files"))
        .map(|x| x.file_name().to_string_lossy().to_string())
        .collect()
}

pub fn to_set<T: AsRef<str>>(v: &[T]) -> HashSet<String> {
    v.iter()
        .map(|s| s.as_ref().to_string())
        .collect::<HashSet<String>>()
}

fn unpack_cert_resources(cert_resources_dir: &PathBuf) {
    let mut cert_archive = test_resources_dir_path();
    cert_archive.push("certificates.tar");
    let cert_archive = File::open(cert_archive).expect("Can open cert archive file");
    let mut cert_archive = Archive::new(cert_archive);
    cert_archive
        .unpack(cert_resources_dir)
        .expect("Can unack cert archive");
}

pub fn test_resources_dir_path() -> PathBuf {
    let mut test_resources = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    test_resources.push("resources/test");
    test_resources
}

fn resource_cert_dir_path() -> PathBuf {
    let mut cert_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    cert_dir.push("cert_resources");
    cert_dir
}

fn store_cert_dir_path() -> PathBuf {
    let mut cert_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    cert_dir.push("cert_dir");
    cert_dir
}
