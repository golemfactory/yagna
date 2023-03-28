use std::collections::HashMap;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;
use serial_test::serial;
use test_case::test_case;

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

#[test_case(
    vec!["foo_ca-chain.cert.pem"],
    vec!["all"],
    false,
    vec![("fe4f04e2", "none"), ("55e451bd", "all")];
    "Add certificates specifying permissions without `--whole-chain` flag"
)]
#[test_case(
    vec!["foo_ca-chain.cert.pem"],
    vec![],
    false,
    vec![("fe4f04e2", "none"), ("55e451bd", "none")];
    "No permissions specified"
)]
#[test_case(
    vec!["foo_ca-chain.cert.pem"],
    vec!["all"],
    true,
    vec![("fe4f04e2", "all"), ("55e451bd", "all")];
    "Add certificates specifying permissions with `--whole-chain` flag"
)]
#[test_case(
    vec!["foo_ca-chain.cert.pem"],
    vec!["all", "outbound-manifest"],
    false,
    vec![("fe4f04e2", "none"), ("55e451bd", "all")];
    "If `all` permission is specified, all other permissions are ignored"
)]
#[test_case(
    vec!["foo_ca-chain.cert.pem", "foo_req.cert.pem"],
    vec!["outbound-manifest"],
    false,
    vec![("fe4f04e2", "none"), ("55e451bd", "none"), ("25b9430c", "outbound-manifest")];
    "Add longer permissions chain"
)]
#[test_case(
    vec!["foo_ca-chain.cert.pem", "dummy_inter.cert.pem"],
    vec!["outbound-manifest"],
    false,
    vec![("fe4f04e2", "none"), ("55e451bd", "outbound-manifest"), ("20e9eb45", "outbound-manifest")];
    "Add multiple certificates and check permissions"
)]
#[test_case(
    vec!["root-certificate.signed.json", "partner-certificate.signed.json"],
    vec![],
    false,
    vec![("80c84b27", "all"), ("cb16a2ed", "{\"outbound\":\"unrestricted\"}")];
    "Add multiple Golem certificates and check permissions"
)]
#[test_case(
    vec!["foo_req.cert.pem", "partner-certificate.signed.json"],
    vec![],
    false,
    vec![("25b9430c", "none"), ("cb16a2ed", "{\"outbound\":\"unrestricted\"}")];
    "Add Golem and X509 certificate and then check permissions"
)]
#[serial]
fn test_keystore_add_certificate_permissions(
    certificates: Vec<&str>,
    permissions: Vec<&str>,
    whole_chain: bool,
    expected: Vec<(&str, &str)>,
) {
    let result = add_and_list(certificates, permissions, whole_chain);
    for (cert_id, perm) in expected {
        assert_eq!(read_permissions(&result, dbg!(cert_id)), perm);
    }
}

#[serial]
#[test]
fn test_keystore_set_should_modify_existing_permissions() {
    let (resource_cert_dir, cert_dir) = CERT_TEST_RESOURCES.init_cert_dirs();

    add(
        vec!["foo_ca-chain.cert.pem"],
        vec!["all"],
        true,
        &resource_cert_dir,
        &cert_dir,
    );

    // This call doesn't specify any permissions, so these should be removed.
    add(
        vec!["foo_ca-chain.cert.pem"],
        vec![],
        true,
        &resource_cert_dir,
        &cert_dir,
    );

    let result = list_certificates_command(&cert_dir).unwrap();

    assert_eq!(read_permissions(&result, "55e451bd"), "none");
    assert_eq!(read_permissions(&result, "fe4f04e2"), "none");
}

#[serial]
#[test]
fn test_keystore_remove_certificate_check_permissions() {
    let (resource_cert_dir, cert_dir) = CERT_TEST_RESOURCES.init_cert_dirs();

    add(
        vec!["foo_ca-chain.cert.pem", "foo_req.cert.pem"],
        vec!["outbound-manifest"],
        true,
        &resource_cert_dir,
        &cert_dir,
    );

    // This call doesn't specify any permissions, so the will be removed.
    remove(&cert_dir, vec!["fe4f04e2"]);

    let result = list_certificates_command(&cert_dir).unwrap();

    assert_eq!(read_permissions(&result, "55e451bd"), "outbound-manifest");
    assert_eq!(read_permissions(&result, "25b9430c"), "outbound-manifest");
}

#[serial]
#[test]
fn test_add_and_remove_certificates() {
    let (resource_cert_dir, cert_dir) = CERT_TEST_RESOURCES.init_cert_dirs();
    add(
        vec!["foo_req.cert.pem", "partner-certificate.signed.json"],
        vec![],
        false,
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
    let result = add_and_list(
        vec!["foo_req.cert.pem", "partner-certificate.signed.json"],
        vec![],
        false,
    );
    assert_eq!(read_not_after(&result, "cb16a2ed"), "2025-01-01T00:00:00Z");
    assert_eq!(read_not_after(&result, "25b9430c"), "2122-07-17T12:05:22Z")
}

#[serial]
#[test]
fn verify_subject_format() {
    let result = add_and_list(
        vec!["foo_req.cert.pem", "partner-certificate.signed.json"],
        vec![],
        false,
    );
    assert_eq!(read_subject(&result, "cb16a2ed"), "Example partner cert");
    assert_eq!(read_subject(&result, "25b9430c"), "{\"C\":\"CZ\",\"CN\":\"Foo Req\",\"E\":\"office@req.foo.com\",\"O\":\"Foo Req Co\",\"OU\":\"Foo Req HQ\",\"ST\":\"Bohemia\"}")
}

fn add_and_list(
    certificates: Vec<&str>,
    permissions: Vec<&str>,
    whole_chain: bool,
) -> HashMap<String, Value> {
    let (resource_cert_dir, cert_dir) = CERT_TEST_RESOURCES.init_cert_dirs();
    add(
        certificates,
        permissions,
        whole_chain,
        &resource_cert_dir,
        &cert_dir,
    );
    list_certificates_command(&cert_dir).unwrap()
}

fn add(
    certificates: Vec<&str>,
    permissions: Vec<&str>,
    whole_chain: bool,
    resource_cert_dir: &Path,
    cert_dir: &Path,
) {
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

    if !permissions.is_empty() {
        command.arg("--permissions");
        permissions.iter().for_each(|permission| {
            command.arg(permission);
        })
    }

    if whole_chain {
        command.arg("--whole-chain");
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

fn read_permissions(certs: &HashMap<String, Value>, id: &str) -> String {
    read_field(certs, id, "Permissions")
}

fn read_subject(certs: &HashMap<String, Value>, id: &str) -> String {
    read_field(certs, id, "Subject")
}

fn read_field(certs: &HashMap<String, Value>, id: &str, field: &str) -> String {
    let permissions = certs[id].get(field).unwrap();
    if permissions.is_string() {
        // Calling `to_string` would result in quoted string.
        permissions.as_str().unwrap().into()
    } else {
        permissions.to_string()
    }
}
