mod common;

use std::fs;

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
    "Does not fail when listing empty store"
)]
#[test_case(
    &["foo_ca.cert.pem"],
    &[],
    &["c128af8c6d0ba34d940582c01443911d"],
    &["foo_ca.cert.pem"]; 
    "Can load one certificate"
)]
#[test_case(
    &["foo_ca.cert.pem", "foo_inter.cert.pem"],
    &["c128af8c6d0ba34d940582c01443911d", "4e0df976b534cb73794a7613b31af51c"],
    &[],
    &[];
    "Can remove all certificates"
)]
#[test_case(
    &["foo_ca-chain.cert.pem"],
    &[],
    &["4e0df976b534cb73794a7613b31af51c", "c128af8c6d0ba34d940582c01443911d"],
    &["foo_ca-chain.cert.pem"]; 
    "Load keychain loads two certificates and stores them in received form (single keychain file)"
)]
#[test_case(
    &["foo_ca-chain.cert.pem"],
    &["c128af8c6d0ba34d940582c01443911d"],
    &["4e0df976b534cb73794a7613b31af51c"],
    &["foo_ca-chain.cert.4e0df976b534cb73794a7613b31af51c.pem"]; 
    "Load keychain and remove root CA results with intermediate cert and cert file with id in the name"
)]
#[test_case(
    &["foo_ca-chain.cert.pem"], 
    &["4e0df976b534cb73794a7613b31af51c"], 
    &["c128af8c6d0ba34d940582c01443911d"], 
    &["foo_ca-chain.cert.c128af8c6d0ba34d940582c01443911d.pem"]; 
    "Load keychain and remove intermediate cert results with root CA and cert file with id in the name"
)]
#[test_case(
    &["foo_ca.cert.pem", "foo_ca.cert.pem", "foo_ca.cert.pem"], 
    &[],
    &["c128af8c6d0ba34d940582c01443911d"],
    &["foo_ca.cert.pem"]; 
    "Adding duplicates results in a single certificate in the store"
)]
#[serial]
fn certificate_store_test(
    certs_to_add: &[&str],
    ids_to_remove: &[&str],
    expected_ids: &[&str],
    expected_files: &[&str],
) {
    // Having
    let (resource_cert_dir, test_cert_dir) = init_cert_dirs();
    load_certificates(&resource_cert_dir, &test_cert_dir, certs_to_add);
    remove_certificates(&test_cert_dir, ids_to_remove);
    let mut visitor = TestCertDataVisitor::new(expected_ids);
    // When
    visitor = visit_certificates(&test_cert_dir, visitor).expect("Can visit loaded certificates");
    // Then
    visitor.test();
    let certs = loaded_cert_files();
    assert_eq!(certs, to_set(expected_files));
}

/// Name collision should be resolved
#[test]
fn certificate_name_collision_test() {
    // Having
    let (resource_cert_dir, test_cert_dir) = init_cert_dirs();
    let colliding_name = "foo_inter.cert.pem";
    let mut duplicated_name_1 = resource_cert_dir.clone();
    duplicated_name_1.push(colliding_name);
    let mut duplicated_name_2 = resource_cert_dir.clone();
    duplicated_name_2.push("copy");
    fs::create_dir_all(&duplicated_name_2).expect("Can create dir");
    duplicated_name_2.push(colliding_name);
    let mut other = resource_cert_dir.clone();
    other.push("foo_req.cert.pem");
    fs::copy(other, duplicated_name_2).expect("Can copy file");
    load_certificates(
        &resource_cert_dir,
        &test_cert_dir,
        &[colliding_name, &format!("copy/{colliding_name}")],
    );
    let mut visitor = TestCertDataVisitor::new(&[
        "4e0df976b534cb73794a7613b31af51c",
        "0e136cb330db24e5630e64ad46489557",
    ]);
    // When
    visitor = visit_certificates(&test_cert_dir, visitor).expect("Can visit loaded certificates");
    // Then
    visitor.test();
    let certs = loaded_cert_files();
    assert_eq!(certs, to_set(&["foo_req.cert.pem", "foo_req.cert.0.pem"]));
}
