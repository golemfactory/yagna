use utils::*;

use ya_manifest_utils::Keystore;

//Open Points:
//refactor utils functions
//rename tests
//validate only private functions
//add test valid_not_before

#[test]
fn accept_not_expired_certificate() {
    let test_cert_dir = tempfile::tempdir().unwrap();

    let valid_from = chrono::Utc::now();

    let valid_to = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::days(2))
        .unwrap();

    let (self_signed_cert, ca_key_pair) =
        create_self_signed_certificate(valid_from, valid_to).unwrap();

    write_cert_to_file(
        &self_signed_cert,
        &test_cert_dir.path().join("self_signed.pem"),
    );

    let sut = Keystore::load(&test_cert_dir).unwrap();

    let (req, csr_key_pair) = create_csr().unwrap();
    let signed_cert = sign_csr(req, csr_key_pair, &self_signed_cert, ca_key_pair).unwrap();

    assert!(sut
        .verify_cert(base64::encode(signed_cert.to_pem().unwrap()))
        .is_ok());
}

#[test]
fn not_accept_expired_certificate() {
    let test_cert_dir = tempfile::tempdir().unwrap();

    let valid_from = chrono::Utc::now()
        .checked_sub_signed(chrono::Duration::days(2))
        .unwrap();

    let valid_to = chrono::Utc::now()
        .checked_sub_signed(chrono::Duration::days(1))
        .unwrap();

    let (self_signed_cert, ca_key_pair) =
        create_self_signed_certificate(valid_from, valid_to).unwrap();

    write_cert_to_file(
        &self_signed_cert,
        &test_cert_dir.path().join("self_signed.pem"),
    );

    let sut = Keystore::load(&test_cert_dir).unwrap();

    let (req, csr_key_pair) = create_csr().unwrap();
    let signed_cert = sign_csr(req, csr_key_pair, &self_signed_cert, ca_key_pair).unwrap();

    assert!(sut
        .verify_cert(base64::encode(signed_cert.to_pem().unwrap()))
        .is_err());
}

#[test]
fn not_accept_not_ready_certificate() {
    let test_cert_dir = tempfile::tempdir().unwrap();

    let valid_from = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::days(1))
        .unwrap();

    let valid_to = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::days(2))
        .unwrap();

    let (self_signed_cert, ca_key_pair) =
        create_self_signed_certificate(valid_from, valid_to).unwrap();

    write_cert_to_file(
        &self_signed_cert,
        &test_cert_dir.path().join("self_signed.pem"),
    );

    let sut = Keystore::load(&test_cert_dir).unwrap();

    let (req, csr_key_pair) = create_csr().unwrap();
    let signed_cert = sign_csr(req, csr_key_pair, &self_signed_cert, ca_key_pair).unwrap();

    assert!(sut
        .verify_cert(base64::encode(signed_cert.to_pem().unwrap()))
        .is_err());
}

mod utils {
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;

    use chrono::{DateTime, Utc};
    use openssl::asn1::Asn1Time;
    use openssl::bn::{BigNum, MsbOption};
    use openssl::error::ErrorStack;
    use openssl::hash::MessageDigest;
    use openssl::pkey::{PKey, Private};
    use openssl::rsa::Rsa;
    use openssl::x509::extension::{
        AuthorityKeyIdentifier, BasicConstraints, KeyUsage, SubjectAlternativeName,
        SubjectKeyIdentifier,
    };
    use openssl::x509::{X509NameBuilder, X509Ref, X509Req, X509ReqBuilder, X509};

    pub fn write_cert_to_file(cert: &X509Ref, file_path: &Path) {
        let mut file = File::create(&file_path).unwrap();
        file.write_all(&cert.to_pem().unwrap()).unwrap();
    }

    pub fn create_self_signed_certificate(
        valid_from: DateTime<Utc>,
        valid_to: DateTime<Utc>,
    ) -> Result<(X509, PKey<Private>), ErrorStack> {
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

        let not_before = Asn1Time::from_unix(valid_from.timestamp())?;
        cert_builder.set_not_before(&not_before)?;

        let not_after = Asn1Time::from_unix(valid_to.timestamp())?;
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
    pub fn create_csr() -> Result<(X509Req, PKey<Private>), ErrorStack> {
        let rsa = Rsa::generate(2048)?;
        let key_pair = PKey::from_rsa(rsa)?;

        let mut req_builder = X509ReqBuilder::new()?;
        req_builder.set_pubkey(&key_pair)?;

        let mut x509_name = X509NameBuilder::new()?;
        x509_name.append_entry_by_text("C", "US")?;
        x509_name.append_entry_by_text("ST", "TX")?;
        x509_name.append_entry_by_text("O", "Some organization")?;
        x509_name.append_entry_by_text("CN", "www.example.com")?;
        let x509_name = x509_name.build();
        req_builder.set_subject_name(&x509_name)?;

        req_builder.sign(&key_pair, MessageDigest::sha256())?;
        let req = req_builder.build();
        Ok((req, key_pair))
    }

    pub fn sign_csr(
        csr: X509Req,
        csr_keys: PKey<Private>,
        signing_cert: &X509Ref,
        signing_keys: PKey<Private>,
    ) -> Result<X509, ErrorStack> {
        let mut cert_builder = X509::builder()?;
        cert_builder.set_version(2)?;
        let serial_number = {
            let mut serial = BigNum::new()?;
            serial.rand(159, MsbOption::MAYBE_ZERO, false)?;
            serial.to_asn1_integer()?
        };
        cert_builder.set_serial_number(&serial_number)?;
        cert_builder.set_subject_name(csr.subject_name())?;
        cert_builder.set_issuer_name(signing_cert.subject_name())?;
        cert_builder.set_pubkey(&csr_keys)?;
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

        let subject_key_identifier = SubjectKeyIdentifier::new()
            .build(&cert_builder.x509v3_context(Some(signing_cert), None))?;
        cert_builder.append_extension(subject_key_identifier)?;

        let auth_key_identifier = AuthorityKeyIdentifier::new()
            .keyid(false)
            .issuer(false)
            .build(&cert_builder.x509v3_context(Some(signing_cert), None))?;
        cert_builder.append_extension(auth_key_identifier)?;

        let subject_alt_name = SubjectAlternativeName::new()
            .dns("*.example.com")
            .dns("hello.com")
            .build(&cert_builder.x509v3_context(Some(signing_cert), None))?;
        cert_builder.append_extension(subject_alt_name)?;

        cert_builder.sign(&signing_keys, MessageDigest::sha256())?;
        let cert = cert_builder.build();

        Ok(cert)
    }
}
