#[macro_use]
extern crate serial_test;

use std::fs::File;
use std::io::Write;
use std::ops::Add;
use std::{fs, path::PathBuf};

use openssl::asn1::Asn1Time;
use openssl::bn::{BigNum, MsbOption};
use openssl::error::ErrorStack;
use openssl::hash::MessageDigest;
use openssl::pkey::{PKey, PKeyRef, Private};
use openssl::rsa::Rsa;
use openssl::x509::extension::{
    AuthorityKeyIdentifier, BasicConstraints, KeyUsage, SubjectAlternativeName,
    SubjectKeyIdentifier,
};
use openssl::x509::{X509NameBuilder, X509Ref, X509Req, X509ReqBuilder, X509VerifyResult, X509};
use ya_manifest_test_utils::*;
use ya_manifest_utils::Keystore;

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
    let err = result.expect_err("Error result");
    let msg = format!("{err:?}");
    assert_eq!(msg, "Invalid certificate");
}

#[test]
#[serial]
fn accept_not_expired_certificate() {
    let (_, test_cert_dir) = TEST_RESOURCES.init_cert_dirs(); //TODO Rafał move tests to other module & use tempdir

    let (ca_cert, ca_key_pair) = mk_ca_cert().unwrap();

    let (cert_2, _key_pair) = mk_ca_signed_cert(&ca_cert, &ca_key_pair).unwrap();

    let cert_1 = ca_cert.to_pem().unwrap();

    let cert_1_path = test_cert_dir.join("cert1.pem");
    let mut cert_1_file = File::create(&cert_1_path).unwrap();
    cert_1_file.write_all(&cert_1).unwrap();

    let sut = Keystore::load(&test_cert_dir).unwrap();

    let b64_cert = base64::encode(cert_2.to_pem().unwrap());

    std::thread::sleep(std::time::Duration::from_secs(1));

    sut.verify_cert(b64_cert).unwrap();
    // assert!(sut.verify_cert(b64_cert).is_ok());
}

struct SignedRequest {
    cert: String,
    sig: String,
    sig_alg: String,
    data: String,
}

fn prepare_request(resource_cert_dir: PathBuf) -> SignedRequest {
    let resource_dir = TEST_RESOURCES.test_resources_dir_path();

    let mut cert = resource_cert_dir;
    cert.push("foo_req.cert.pem");
    let mut cert = fs::read_to_string(cert).expect("Can read certificate file");
    cert = base64::encode(cert);

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

fn mk_ca_cert() -> Result<(X509, PKey<Private>), ErrorStack> {
    let rsa = Rsa::generate(2048)?;
    let key_pair = PKey::from_rsa(rsa)?;

    let mut x509_name = X509NameBuilder::new()?;
    x509_name.append_entry_by_text("C", "US")?;
    x509_name.append_entry_by_text("ST", "TX")?;
    x509_name.append_entry_by_text("O", "Some CA organization")?;
    x509_name.append_entry_by_text("CN", "ca test")?;
    let x509_name = x509_name.build();

    let mut cert_builder = X509::builder()?;
    cert_builder.set_version(2)?;
    let serial_number = {
        let mut serial = BigNum::new()?;
        serial.rand(159, MsbOption::MAYBE_ZERO, false)?;
        serial.to_asn1_integer()?
    };
    cert_builder.set_serial_number(&serial_number)?;
    cert_builder.set_subject_name(&x509_name)?;
    cert_builder.set_issuer_name(&x509_name)?;
    cert_builder.set_pubkey(&key_pair)?;
    let not_before = Asn1Time::days_from_now(0)?;
    cert_builder.set_not_before(&not_before)?;

    let now = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::seconds(3))
        .unwrap();

    let not_after = Asn1Time::from_unix(now.timestamp())?;
    cert_builder.set_not_after(&not_after)?;

    cert_builder.append_extension(BasicConstraints::new().critical().ca().build()?)?;
    cert_builder.append_extension(
        KeyUsage::new()
            .critical()
            .key_cert_sign()
            .crl_sign()
            .build()?,
    )?;

    let subject_key_identifier =
        SubjectKeyIdentifier::new().build(&cert_builder.x509v3_context(None, None))?;
    cert_builder.append_extension(subject_key_identifier)?;

    cert_builder.sign(&key_pair, MessageDigest::sha256())?;
    let cert = cert_builder.build();

    Ok((cert, key_pair))
}

/// Make a X509 request with the given private key
fn mk_request(key_pair: &PKey<Private>) -> Result<X509Req, ErrorStack> {
    let mut req_builder = X509ReqBuilder::new()?;
    req_builder.set_pubkey(key_pair)?;

    let mut x509_name = X509NameBuilder::new()?;
    x509_name.append_entry_by_text("C", "US")?;
    x509_name.append_entry_by_text("ST", "TX")?;
    x509_name.append_entry_by_text("O", "Some organization")?;
    x509_name.append_entry_by_text("CN", "www.example.com")?;
    let x509_name = x509_name.build();
    req_builder.set_subject_name(&x509_name)?;

    req_builder.sign(key_pair, MessageDigest::sha256())?;
    let req = req_builder.build();
    Ok(req)
}

/// Make a certificate and private key signed by the given CA cert and private key
fn mk_ca_signed_cert(
    ca_cert: &X509Ref,
    ca_key_pair: &PKeyRef<Private>,
) -> Result<(X509, PKey<Private>), ErrorStack> {
    let rsa = Rsa::generate(2048)?;
    let key_pair = PKey::from_rsa(rsa)?;

    let req = mk_request(&key_pair)?;

    let mut cert_builder = X509::builder()?;
    cert_builder.set_version(2)?;
    let serial_number = {
        let mut serial = BigNum::new()?;
        serial.rand(159, MsbOption::MAYBE_ZERO, false)?;
        serial.to_asn1_integer()?
    };
    cert_builder.set_serial_number(&serial_number)?;
    cert_builder.set_subject_name(req.subject_name())?;
    cert_builder.set_issuer_name(ca_cert.subject_name())?;
    cert_builder.set_pubkey(&key_pair)?;
    let not_before = Asn1Time::days_from_now(0)?;
    cert_builder.set_not_before(&not_before)?;
    let not_after = Asn1Time::days_from_now(365)?;
    cert_builder.set_not_after(&not_after)?;

    cert_builder.append_extension(BasicConstraints::new().build()?)?;

    cert_builder.append_extension(
        KeyUsage::new()
            .critical()
            .non_repudiation()
            .digital_signature()
            .key_encipherment()
            .build()?,
    )?;

    let subject_key_identifier =
        SubjectKeyIdentifier::new().build(&cert_builder.x509v3_context(Some(ca_cert), None))?;
    cert_builder.append_extension(subject_key_identifier)?;

    let auth_key_identifier = AuthorityKeyIdentifier::new()
        .keyid(false)
        .issuer(false)
        .build(&cert_builder.x509v3_context(Some(ca_cert), None))?;
    cert_builder.append_extension(auth_key_identifier)?;

    let subject_alt_name = SubjectAlternativeName::new()
        .dns("*.example.com")
        .dns("hello.com")
        .build(&cert_builder.x509v3_context(Some(ca_cert), None))?;
    cert_builder.append_extension(subject_alt_name)?;

    cert_builder.sign(ca_key_pair, MessageDigest::sha256())?;
    let cert = cert_builder.build();

    Ok((cert, key_pair))
}
