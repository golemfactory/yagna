use ya_manifest_utils::util;

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeSet, HashSet},
        fs,
        path::PathBuf,
    };

    use ya_manifest_utils::{
        util::{self, CertBasicData, CertBasicDataVisitor},
        Keystore,
    };

    #[test]
    fn load_empty_cert_dir_test() {
        let (_, cert_dir) = init_cert_dirs();
        let mut visitor = TestCertDataVisitor::default();
        visitor =
            util::visit_certificates(&cert_dir, visitor).expect("Visiting certificates works.");
        visitor.test();
        let certs = loaded_cert_files();
        assert!(certs.is_empty(), "Cert dir is not empty");
    }

    #[test]
    fn load_one_certificate() {
        let (resource_cert_dir, test_cert_dir) = init_cert_dirs();
        load_certificates(&resource_cert_dir, &test_cert_dir, vec!["foo_ca.cert.pem"]);
        Keystore::load(&test_cert_dir).expect("Certificates can be loadeded from Keystore");
        let mut visitor = TestCertDataVisitor::new(vec!["c128af8c6d0ba34d940582c01443911d"]);
        visitor = util::visit_certificates(&test_cert_dir, visitor)
            .expect("Can visit loaded certificates");
        visitor.test();
        let certs = loaded_cert_files();
        assert_eq!(certs, vec_to_set(vec!["foo_ca.cert.pem"]));
    }

    fn load_certificates(
        resource_cert_dir: &PathBuf,
        test_cert_dir: &PathBuf,
        certfile_names: Vec<&str>,
    ) {
        let cert_paths: Vec<PathBuf> = certfile_names
            .iter()
            .map(|certfile_name| {
                let mut cert_path = resource_cert_dir.clone();
                cert_path.push(certfile_name);
                return cert_path;
            })
            .collect();
        let keystore_manager =
            util::KeystoreManager::try_new(&test_cert_dir).expect("Can createt keystore manager");
        keystore_manager
            .load_certs(&cert_paths)
            .expect("Can load certificates");
    }

    #[derive(Default)]
    struct TestCertDataVisitor {
        expected: Vec<String>,
        actual: Vec<String>,
    }

    impl TestCertDataVisitor {
        pub fn new(expected: Vec<&str>) -> Self {
            let mut visitor = Self::default();
            visitor.expected = expected.iter().map(|s| s.to_string()).collect();
            visitor
        }

        pub fn test(&self) {
            assert_eq!(self.expected, self.actual)
        }
    }

    impl CertBasicDataVisitor for TestCertDataVisitor {
        fn accept(&mut self, cert_data: CertBasicData) {
            self.actual.push(cert_data.id.clone());
        }
    }

    fn init_cert_dirs() -> (PathBuf, PathBuf) {
        let mut resource_cert_dir = resource_cert_dir_path();
        let mut store_cert_dir = store_cert_dir_path();
        if store_cert_dir.exists() {
            fs::remove_dir_all(&store_cert_dir).expect("Can delete test temp dir");
        }
        fs::create_dir_all(&store_cert_dir).expect("Can create temp dir");
        (resource_cert_dir, store_cert_dir)
    }

    fn loaded_cert_files() -> HashSet<String> {
        let store_cert_dir = store_cert_dir_path();
        let cert_dir = fs::read_dir(store_cert_dir).expect("Can read cert dir");
        cert_dir
            .into_iter()
            .map(|file| file.expect("Can list cert files"))
            .map(|x| x.file_name().to_string_lossy().to_string())
            .collect()
    }

    fn resource_cert_dir_path() -> PathBuf {
        let mut cert_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        cert_dir.push("resources/test/certificates");
        cert_dir
    }

    fn store_cert_dir_path() -> PathBuf {
        let mut cert_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        cert_dir.push("cert_dir");
        cert_dir
    }

    fn vec_to_set(v: Vec<&str>) -> HashSet<String> {
        v.iter().map(|s| s.to_string()).collect::<HashSet<String>>()
    }
}
