#[macro_use]
extern crate serial_test;

use std::{collections::HashSet, fs};
use test_case::test_case;
use ya_manifest_test_utils::*;
use ya_manifest_utils::{keystore::Keystore, policy::CertPermissions, CompositeKeystore};

static TEST_RESOURCES: TestResources = TestResources {
    temp_dir: env!("CARGO_TARGET_TMPDIR"),
};

/// Test utilities
#[test_case(
    &[],
    &[],
    &[],
    &["cert-permissions.json"];
    "Does not fail when listing empty store"
)]
#[test_case(
    &["foo_ca.cert.pem"],
    &[],
    &["fe4f04e2517488ba32acd4354fca66fd67b4078722fcd1a17a1487b7723477b62af2f17fa35c9b329b52658ccfbbe663c64c3fdf4378519d9279c13e88f7bb99"],
    &["foo_ca.cert.pem", "cert-permissions.json"]; 
    "Can load one certificate"
)]
#[test_case(
    &["foo_ca.cert.pem", "foo_inter.cert.pem"],
    &["fe4f04e2517488ba32acd4354fca66fd67b4078722fcd1a17a1487b7723477b62af2f17fa35c9b329b52658ccfbbe663c64c3fdf4378519d9279c13e88f7bb99", "55e451bd1a2f43570a25052b863af1d527fe6fd4bfd1482fdb241596432477f20eb2b2f3801fb5c6cd785f1a03c43ccf71fd8cdf0a974d1296be2326b0824673"],
    &[],
    &["cert-permissions.json"];
    "Can remove all certificates"
)]
#[test_case(
    &["foo_ca-chain.cert.pem"],
    &[],
    &["55e451bd1a2f43570a25052b863af1d527fe6fd4bfd1482fdb241596432477f20eb2b2f3801fb5c6cd785f1a03c43ccf71fd8cdf0a974d1296be2326b0824673", "fe4f04e2517488ba32acd4354fca66fd67b4078722fcd1a17a1487b7723477b62af2f17fa35c9b329b52658ccfbbe663c64c3fdf4378519d9279c13e88f7bb99"],
    &["foo_ca-chain.cert.pem", "cert-permissions.json"]; 
    "Load keychain loads two certificates and stores them in received form (single keychain file)"
)]
#[test_case(
    &["foo_ca-chain.cert.pem"],
    &["fe4f04e2517488ba32acd4354fca66fd67b4078722fcd1a17a1487b7723477b62af2f17fa35c9b329b52658ccfbbe663c64c3fdf4378519d9279c13e88f7bb99"],
    &["55e451bd1a2f43570a25052b863af1d527fe6fd4bfd1482fdb241596432477f20eb2b2f3801fb5c6cd785f1a03c43ccf71fd8cdf0a974d1296be2326b0824673"],
    &["foo_ca-chain.cert.55e451bd1a2f43570a25052b863af1d527fe6fd4bfd1482fdb241596432477f20eb2b2f3801fb5c6cd785f1a03c43ccf71fd8cdf0a974d1296be2326b0824673.pem", "cert-permissions.json"]; 
    "Load keychain and remove root CA results with intermediate cert and cert file with id in the name"
)]
#[test_case(
    &["foo_ca-chain.cert.pem"], 
    &["55e451bd1a2f43570a25052b863af1d527fe6fd4bfd1482fdb241596432477f20eb2b2f3801fb5c6cd785f1a03c43ccf71fd8cdf0a974d1296be2326b0824673"], 
    &["fe4f04e2517488ba32acd4354fca66fd67b4078722fcd1a17a1487b7723477b62af2f17fa35c9b329b52658ccfbbe663c64c3fdf4378519d9279c13e88f7bb99"], 
    &["foo_ca-chain.cert.fe4f04e2517488ba32acd4354fca66fd67b4078722fcd1a17a1487b7723477b62af2f17fa35c9b329b52658ccfbbe663c64c3fdf4378519d9279c13e88f7bb99.pem", "cert-permissions.json"]; 
    "Load keychain and remove intermediate cert results with root CA and cert file with id in the name"
)]
#[test_case(
    &["foo_ca.cert.pem", "foo_ca.cert.pem", "foo_ca.cert.pem"], 
    &[],
    &["fe4f04e2517488ba32acd4354fca66fd67b4078722fcd1a17a1487b7723477b62af2f17fa35c9b329b52658ccfbbe663c64c3fdf4378519d9279c13e88f7bb99"],
    &["foo_ca.cert.pem", "cert-permissions.json"]; 
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
    let (resource_cert_dir, test_cert_dir) = TEST_RESOURCES.init_cert_dirs();
    load_certificates_from_dir(
        &resource_cert_dir,
        &test_cert_dir,
        certs_to_add,
        &vec![CertPermissions::All],
    );
    remove_certificates(&test_cert_dir, ids_to_remove);
    // When
    let keystore = CompositeKeystore::load(&test_cert_dir).expect("Can load keystore");
    let loaded_ids = keystore.list_ids().into_iter().collect::<HashSet<String>>();
    // Then
    let expected_ids = expected_ids
        .iter()
        .map(|id| id.to_string())
        .collect::<HashSet<String>>();
    assert_eq!(expected_ids, loaded_ids);
    let certs = TEST_RESOURCES.loaded_cert_files();
    assert_eq!(certs, slice_to_set(expected_files));
}

/// Name collision should be resolved
#[test]
#[serial]
fn certificate_name_collision_test() {
    // Having
    let (resource_cert_dir, test_cert_dir) = TEST_RESOURCES.init_cert_dirs();
    let colliding_name = "foo_inter.cert.pem";

    let mut colliding_file_1 = resource_cert_dir.clone();
    colliding_file_1.push(colliding_name);

    let mut colliding_file_2 = resource_cert_dir.clone();
    colliding_file_2.push("copy");
    fs::create_dir_all(&colliding_file_2).expect("Can create dir");
    colliding_file_2.push(colliding_name);
    let mut other = resource_cert_dir.clone();
    other.push("foo_req.cert.pem");
    fs::copy(other, colliding_file_2).expect("Can copy file");

    load_certificates_from_dir(
        &resource_cert_dir,
        &test_cert_dir,
        &[colliding_name, &format!("copy/{colliding_name}")],
        &vec![CertPermissions::All],
    );

    let expected_ids: HashSet<String> = HashSet::from(["25b9430c6169c7ae66c9a7d9ec411fd9d50d4264ce4d94d47cf109a6afa6623d46ec90249c3a662adddcf17d162c61b3f07f24d240f21902ebd0e21ac0ecafd1".into(), "55e451bd1a2f43570a25052b863af1d527fe6fd4bfd1482fdb241596432477f20eb2b2f3801fb5c6cd785f1a03c43ccf71fd8cdf0a974d1296be2326b0824673".into()]);
    // When
    let keystore = CompositeKeystore::load(&test_cert_dir).expect("Can laod keystore");
    let loaded_ids = keystore
        .list()
        .into_iter()
        .map(|c| c.id())
        .collect::<HashSet<String>>();
    // Then
    assert_eq!(expected_ids, loaded_ids);
    let certs = TEST_RESOURCES.loaded_cert_files();
    assert_eq!(
        certs,
        slice_to_set(&[
            "foo_inter.cert.pem",
            "foo_inter.cert.0.pem",
            "cert-permissions.json"
        ])
    );
}
