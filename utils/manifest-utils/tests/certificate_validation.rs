mod common;

use std::fs;

use common::*;
use ya_manifest_utils::Keystore;

extern crate serial_test;

#[test]
fn validation_test() {
    // Having
    let resource_dir = common::test_resources_dir_path();
    let (resource_cert_dir, test_cert_dir) = init_cert_dirs();
    load_certificates(
        &resource_cert_dir,
        &test_cert_dir,
        &["foo_ca-chain.cert.pem"],
    );

    let mut cert = resource_cert_dir.clone();
    cert.push("foo_req.cert.pem");
    let mut cert = fs::read_to_string(cert).expect("Can read certificate file");
    cert = base64::encode(cert);

    let mut data = resource_dir.clone();
    data.push("data.json.base64");
    let data = fs::read_to_string(data).expect("Can read resource file");

    let mut sig = resource_dir.clone();
    sig.push("data.json.base64.foo_req_sign.sha256.base64");
    let sig = fs::read_to_string(sig).expect("Can read resource file");

    let sig_alg = "sha256";

    // Then
    let keystore = Keystore::load(&test_cert_dir).expect("Can load certificates");
    keystore
        .verify_signature(cert, sig, sig_alg, data)
        .expect("Signature and cert can be validated")
}
