mod common;

use test_case::test_case;

use common::*;
use ya_manifest_utils::util::visit_certificates;

#[macro_use]
extern crate serial_test;

/// Test utilities
#[test_case(
    &[],
    &[],
    &[],
    &[]; 
    "Load empty"
)]
#[test_case(
    &["foo_ca.cert.pem"],
    &[],
    &["c128af8c6d0ba34d940582c01443911d"],
    &["foo_ca.cert.pem"]; 
    "Load one certificate"
)]
#[test_case(
    &["foo_ca-chain.cert.pem"],
    &[],
    &["4e0df976b534cb73794a7613b31af51c", "c128af8c6d0ba34d940582c01443911d"],
    &["foo_ca-chain.cert.pem"]; 
    "Load keychain"
)]
#[test_case(
    &["foo_ca-chain.cert.pem"],
    &["4e0df976b534cb73794a7613b31af51c"],
    &["c128af8c6d0ba34d940582c01443911d"],
    &["foo_ca-chain.cert.c128af8c6d0ba34d940582c01443911d.pem"]; 
    "Load keychain and remove intermediate"
)]
#[test_case(
    &["foo_ca-chain.cert.pem"],
    &["c128af8c6d0ba34d940582c01443911d"],
    &["4e0df976b534cb73794a7613b31af51c"],
    &["foo_ca-chain.cert.4e0df976b534cb73794a7613b31af51c.pem"]; 
    "Load keychain and remove root"
)]
#[test_case(
    &["foo_ca-chain.cert.pem"], 
    &["4e0df976b534cb73794a7613b31af51c"], 
    &["c128af8c6d0ba34d940582c01443911d"], 
    &["foo_ca-chain.cert.c128af8c6d0ba34d940582c01443911d.pem"]; 
    "Load keychain and remove intermediate cert"
)]
#[serial]
fn certificate_store_test(
    certs_to_add: &[&str],
    ids_to_remove: &[&str],
    expected_ids: &[&str],
    expected_files: &[&str],
) {
    let (resource_cert_dir, test_cert_dir) = init_cert_dirs();
    load_certificates(&resource_cert_dir, &test_cert_dir, certs_to_add);
    remove_certificates(&test_cert_dir, ids_to_remove);
    let mut visitor = TestCertDataVisitor::new(expected_ids);
    visitor = visit_certificates(&test_cert_dir, visitor)
        .expect("Can visit loaded certificates");
    visitor.test();
    let certs = loaded_cert_files();
    assert_eq!(certs, to_set(expected_files));
}
