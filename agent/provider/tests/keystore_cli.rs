use std::collections::HashMap;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serial_test::serial;

use ya_manifest_test_utils::TestResources;

static CERT_TEST_RESOURCES: TestResources = TestResources {
    temp_dir: env!("CARGO_TARGET_TMPDIR"),
};

fn prepare_test_dirs() -> (PathBuf, PathBuf) {
    let mut data_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    data_dir.push("data-dir");
    let mut cert_dir = data_dir.clone();
    cert_dir.push("cert-dir");
    #[allow(unused_must_use)]
    {
        std::fs::remove_dir_all(&data_dir); // ignores error if does not exist
        std::fs::remove_dir_all(&cert_dir); // ignores error if does not exist
    }

    (data_dir, cert_dir)
}

#[serial]
#[test]
fn test_keystore_list_cmd_creates_cert_dir_in_data_dir_set_by_env() {
    // Having
    let (data_dir, cert_dir) = prepare_test_dirs();

    // When
    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.as_path().to_str().unwrap())
        .arg("keystore")
        .arg("list")
        .arg("--json")
        .assert()
        .stdout("[]\n")
        .success();
    // Then
    assert!(
        cert_dir.exists(),
        "Cert dir has been created inside of data dir"
    );
}

#[serial]
#[test]
fn test_keystore_list_cmd_creates_cert_dir_in_dir_set_by_env() {
    // Having
    let (data_dir, cert_dir) = prepare_test_dirs();

    let mut wrong_cert_dir = data_dir.clone();
    wrong_cert_dir.push("cert_dir");
    // When
    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.as_path().to_str().unwrap())
        .env("PROVIDER_CERT_DIR", cert_dir.as_path().to_str().unwrap())
        .arg("keystore")
        .arg("list")
        .arg("--json")
        .assert()
        .stdout("[]\n")
        .success();
    // Then
    assert!(
        cert_dir.exists(),
        "Cert dir has been created inside of dir pointed by $PROVIDER_CERT_DIR"
    );
    assert!(
        !wrong_cert_dir.exists(),
        "Cert dir has not been created inside of data dir pointed by $DATA_DIR"
    );
}

#[serial]
#[test]
fn test_keystore_list_cmd_creates_cert_dir_in_dir_set_by_arg() {
    // Having
    let (data_dir, cert_dir) = prepare_test_dirs();

    // When
    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.as_path().to_str().unwrap())
        .args(["--cert-dir", cert_dir.as_path().to_str().unwrap()])
        .arg("keystore")
        .arg("list")
        .arg("--json")
        .assert()
        .stdout("[]\n")
        .success();
    // Then
    assert!(
        cert_dir.exists(),
        "Cert dir has been created inside of dir pointed by --cert-dir arg"
    );
}

#[serial]
#[test]
fn test_add_and_remove_certificates() {
    let (resource_cert_dir, cert_dir) = CERT_TEST_RESOURCES.init_cert_dirs();
    add(
        vec!["foo_req.cert.pem", "partner-certificate.signed.json"],
        &resource_cert_dir,
        &cert_dir,
    );

    let result = list_certificates_command(&cert_dir).unwrap();
    assert!(result.contains_key("cb16a2ed"));
    assert!(result.contains_key("25b9430c"));
    assert_eq!(result.len(), 2);

    remove(&cert_dir, vec!["cb16a2ed", "25b9430c"]);

    let result = list_certificates_command(&cert_dir).unwrap();
    assert!(result.is_empty());
}

#[serial]
#[test]
fn verify_not_after_date_format() {
    let result = add_and_list(vec!["foo_req.cert.pem", "partner-certificate.signed.json"]);
    assert_eq!(read_not_after(&result, "cb16a2ed"), "2025-01-01T00:00:00Z");
    assert_eq!(read_not_after(&result, "25b9430c"), "2122-07-17T12:05:22Z")
}

#[serial]
#[test]
fn verify_subject_format() {
    let result = add_and_list(vec!["foo_req.cert.pem", "partner-certificate.signed.json"]);
    assert_eq!(read_subject(&result, "cb16a2ed"), "Example partner cert");
    assert_eq!(read_subject(&result, "25b9430c"), "{\"CN\":\"Foo Req\",\"E\":\"office@req.foo.com\",\"O\":\"Foo Req Co\",\"OU\":\"Foo Req HQ\"}")
}

#[serial]
#[test]
fn verify_outbound_rules_format() {
    let (resource_cert_dir, cert_dir) = CERT_TEST_RESOURCES.init_cert_dirs();
    add(
        vec!["foo_req.cert.pem", "partner-certificate.signed.json"],
        &resource_cert_dir,
        &cert_dir,
    );
    let result = list_certificates_command(&cert_dir).unwrap();
    assert_eq!(read_outbound_rules(&result, "cb16a2ed"), "");
    assert_eq!(read_outbound_rules(&result, "25b9430c"), "");

    set_partner_rule(&cert_dir, "cb16a2ed");
    let result = list_certificates_command(&cert_dir).unwrap();
    assert_eq!(read_outbound_rules(&result, "cb16a2ed"), "Partner");
}

fn set_partner_rule(cert_dir: &Path, cert: &str) {
    Command::cargo_bin("ya-provider")
        .unwrap()
        .args(["--cert-dir", cert_dir.to_str().unwrap()])
        .arg("rule")
        .arg("set")
        .arg("outbound")
        .arg("partner")
        .arg("cert-id")
        .arg(cert)
        .arg("--mode")
        .arg("all")
        .assert()
        .success();
}

fn add_and_list(certificates: Vec<&str>) -> HashMap<String, Value> {
    let (resource_cert_dir, cert_dir) = CERT_TEST_RESOURCES.init_cert_dirs();
    add(certificates, &resource_cert_dir, &cert_dir);
    list_certificates_command(&cert_dir).unwrap()
}

fn add(certificates: Vec<&str>, resource_cert_dir: &Path, cert_dir: &Path) {
    let mut command = Command::cargo_bin("ya-provider").unwrap();
    command
        .args(["--cert-dir", cert_dir.to_str().unwrap()])
        .arg("keystore")
        .arg("add");

    if !certificates.is_empty() {
        for certificate in certificates {
            command.arg(resource_cert_dir.join(certificate));
        }
    }

    command.arg("--json").assert().success();
}

fn remove(cert_dir: &Path, certificate_ids: Vec<&str>) {
    let mut command = Command::cargo_bin("ya-provider").unwrap();
    command
        .args(["--cert-dir", cert_dir.to_str().unwrap()])
        .arg("keystore")
        .arg("remove");
    for certificate_id in certificate_ids {
        command.arg(certificate_id);
    }
    command.arg("--json").assert().success();
}

fn list_certificates_command(
    cert_dir: &Path,
) -> anyhow::Result<HashMap<String, serde_json::Value>> {
    let output = Command::cargo_bin("ya-provider")
        .unwrap()
        .args(["--cert-dir", cert_dir.to_str().unwrap()])
        .arg("keystore")
        .arg("list")
        .arg("--json")
        .output()?;
    let result: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    Ok(result
        .as_array()
        .unwrap()
        .iter()
        .map(|element| {
            (
                element
                    .as_object()
                    .unwrap()
                    .get("ID")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string(),
                element.clone(),
            )
        })
        .collect())
}

fn read_not_after(certs: &HashMap<String, Value>, id: &str) -> String {
    read_field(certs, id, "Not After")
}

fn read_subject(certs: &HashMap<String, Value>, id: &str) -> String {
    read_field(certs, id, "Subject")
}

fn read_outbound_rules(certs: &HashMap<String, Value>, id: &str) -> String {
    read_field(certs, id, "Outbound Rules")
}

fn read_field(certs: &HashMap<String, Value>, id: &str, field: &str) -> String {
    let field = certs[id].get(field).unwrap();
    if field.is_string() {
        // Calling `to_string` would result in quoted string.
        field.as_str().unwrap().into()
    } else {
        field.to_string()
    }
}
