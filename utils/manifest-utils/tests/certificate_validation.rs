#[macro_use]
extern crate serial_test;

use std::{fs, path::PathBuf};

use ya_manifest_utils::Keystore;
use ya_manifest_test_utils::*;

static TEST_RESOURCES: TestResources = TestResources { temp_dir: env!("CARGO_TARGET_TMPDIR") };

#[test]
#[serial]
fn valid_certificate_test() {
    // Having
    let (resource_cert_dir, test_cert_dir) = TEST_RESOURCES.init_cert_dirs();
    load_certificates_from_dir(
        &resource_cert_dir,
        &test_cert_dir,
        &["foo_ca-chain.cert.pem"],
    );

    let request = prepare_request(resource_cert_dir);

    // Then
    let keystore = Keystore::load(&test_cert_dir).expect("Can load certificates");
    keystore
        .verify_signature(request.cert, request.sig, request.sig_alg, request.data)
        .expect("Signature and cert can be validated")
}

#[test]
#[serial]
fn invalid_certificate_test() {
    // Having
    let (resource_cert_dir, test_cert_dir) = TEST_RESOURCES.init_cert_dirs();
    load_certificates_from_dir(&resource_cert_dir, &test_cert_dir, &[]);

    let request = prepare_request(resource_cert_dir);

    // Then
    let keystore = Keystore::load(&test_cert_dir).expect("Can load certificates");
    let result =
        keystore.verify_signature(request.cert, request.sig, request.sig_alg, request.data);
    assert!(
        result.is_err(),
        "Keystore has no intermediate cert - verification should fail"
    );
    let err = result.err().expect("Error result");
    let msg = format!("{err:?}");
    assert_eq!(msg, "Invalid certificate");
}

struct SignedRequest {
    cert: String,
    sig: String,
    sig_alg: String,
    data: String,
}

fn prepare_request(resource_cert_dir: PathBuf) -> SignedRequest {
    let resource_dir = TEST_RESOURCES.test_resources_dir_path();

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

    let sig_alg = "sha256".to_string();

    SignedRequest {
        cert,
        sig,
        sig_alg,
        data,
    }
}
