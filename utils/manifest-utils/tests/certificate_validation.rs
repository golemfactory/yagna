#[macro_use]
extern crate serial_test;

use std::{fs, path::PathBuf};

use test_case::test_case;
use ya_manifest_test_utils::*;
use ya_manifest_utils::keystore::x509_keystore::X509Keystore;

static TEST_RESOURCES: TestResources = TestResources {
    temp_dir: env!("CARGO_TARGET_TMPDIR"),
};

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
    let keystore = X509Keystore::load(&test_cert_dir).expect("Can load certificates");
    keystore
        .verify_signature(request.cert, request.sig, request.sig_alg, request.data)
        .expect("Signature and cert can be validated")
}

#[test_case(&[], Some("foo_req.cert.pem"), "Unable to verify X509 certificate. No X509 certificates in keystore."; "Empty keystore failure test")]
#[test_case(&["foo_ca.cert.pem"], Some("foo_req.cert.pem"), "Unable to verify X509 certificate."; "Unable to verify failure test")]
#[test_case(&["foo_ca.cert.pem"], None, "Unable to verify X509 certificate. No X509 certificate in payload."; "No cert in payload failure")]
#[test_case(&[], None, "Unable to verify X509 certificate. No X509 certificate in payload."; "No cert in payload failure (when empty keystore)")]
#[serial]
fn cert_verification_failure_test(
    certificates: &[&str],
    req_cert: Option<&str>,
    expected_error_msg: &str,
) {
    // Having
    let (resource_cert_dir, test_cert_dir) = TEST_RESOURCES.init_cert_dirs();
    load_certificates_from_dir(&resource_cert_dir, &test_cert_dir, certificates);

    let request = prepare_request_parameterized(resource_cert_dir, req_cert);

    // Then
    let keystore = X509Keystore::load(&test_cert_dir).expect("Can load certificates");
    let result =
        keystore.verify_signature(request.cert, request.sig, request.sig_alg, request.data);

    let err = result.expect_err("Error result");
    let msg = format!("{err:?}");
    assert_eq!(msg, expected_error_msg);
}

struct SignedRequest {
    cert: String,
    sig: String,
    sig_alg: String,
    data: String,
}

fn prepare_request(resource_cert_dir: PathBuf) -> SignedRequest {
    prepare_request_parameterized(resource_cert_dir, Some("foo_req.cert.pem"))
}

fn prepare_request_parameterized(
    resource_cert_dir: PathBuf,
    cert_file: Option<&str>,
) -> SignedRequest {
    let resource_dir = TestResources::test_resources_dir_path();

    let cert = match cert_file {
        Some(cert_file) => {
            let mut cert = resource_cert_dir;
            cert.push(cert_file);
            let cert = fs::read_to_string(cert).expect("Can read certificate file");
            base64::encode(cert)
        }
        None => "".to_string(),
    };

    let mut data = resource_dir.clone();
    data.push("data.json.base64");
    let data = fs::read_to_string(data).expect("Can read resource file");

    let mut sig = resource_dir;
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
