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
    vec![("c128af8c", "none"), ("4e0df976", "all")];
    "Add certificates specifying permissions without `--whole-chain` flag"
)]
#[test_case(
    vec!["foo_ca-chain.cert.pem"],
    vec![],
    false,
    vec![("c128af8c", "none"), ("4e0df976", "none")];
    "No permissions specified"
)]
#[test_case(
    vec!["foo_ca-chain.cert.pem"],
    vec!["all"],
    true,
    vec![("c128af8c", "all"), ("4e0df976", "all")];
    "Add certificates specifying permissions with `--whole-chain` flag"
)]
#[test_case(
    vec!["foo_ca-chain.cert.pem"],
    vec!["all", "outbound-manifest"],
    false,
    vec![("c128af8c", "none"), ("4e0df976", "all")];
    "If `all` permission is specified, all other permissions are ignored"
)]
#[test_case(
    vec!["foo_ca-chain.cert.pem", "foo_req.cert.pem"],
    vec!["outbound-manifest"],
    false,
    vec![("c128af8c", "none"), ("4e0df976", "none"), ("0e136cb3", "outbound-manifest")];
    "Add longer permissions chain"
)]
#[test_case(
    vec!["foo_ca-chain.cert.pem", "dummy_inter.cert.pem"],
    vec!["outbound-manifest"],
    false,
    vec![("c128af8c", "none"), ("4e0df976", "outbound-manifest"), ("2e6e701d", "outbound-manifest")];
    "Add multiple certificates and check permissions"
)]
#[serial]
fn test_keystore_add_certificate_permissions(
    certificates: Vec<&str>,
    permissions: Vec<&str>,
    whole_chain: bool,
    expected: Vec<(&str, &str)>,
) {
    let (resource_cert_dir, cert_dir) = CERT_TEST_RESOURCES.init_cert_dirs();

    let mut command = Command::cargo_bin("ya-provider").unwrap();
    command
        .args(["--cert-dir", cert_dir.as_path().to_str().unwrap()])
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

    let result = list_certificates_command(&cert_dir).unwrap();
    println!("Result: {result:?}");
    for (cert_id, perm) in expected {
        assert_eq!(check_permissions(&result, cert_id), perm);
    }
}

#[serial]
#[test]
fn test_keystore_add_certificate_second_time() {
    let (resource_cert_dir, cert_dir) = CERT_TEST_RESOURCES.init_cert_dirs();

    Command::cargo_bin("ya-provider")
        .unwrap()
        .args(["--cert-dir", cert_dir.as_path().to_str().unwrap()])
        .arg("keystore")
        .arg("add")
        .arg(resource_cert_dir.join("foo_ca-chain.cert.pem"))
        .arg("--permissions")
        .arg("all")
        .arg("--whole-chain")
        .arg("--json")
        .assert()
        .success();

    // This call doesn't specify any permissions, so the will be removed.
    Command::cargo_bin("ya-provider")
        .unwrap()
        .args(["--cert-dir", cert_dir.as_path().to_str().unwrap()])
        .arg("keystore")
        .arg("add")
        .arg(resource_cert_dir.join("foo_ca-chain.cert.pem"))
        .arg("--whole-chain")
        .arg("--json")
        .assert()
        .success();

    let result = list_certificates_command(&cert_dir).unwrap();

    assert_eq!(check_permissions(&result, "4e0df976"), "none");
    assert_eq!(check_permissions(&result, "c128af8c"), "none");
}

#[serial]
#[test]
fn test_keystore_remove_certificate_check_permissions() {
    let (resource_cert_dir, cert_dir) = CERT_TEST_RESOURCES.init_cert_dirs();

    Command::cargo_bin("ya-provider")
        .unwrap()
        .args(["--cert-dir", cert_dir.as_path().to_str().unwrap()])
        .arg("keystore")
        .arg("add")
        .arg(resource_cert_dir.join("foo_ca-chain.cert.pem"))
        .arg(resource_cert_dir.join("foo_req.cert.pem"))
        .arg("--permissions")
        .arg("outbound-manifest")
        .arg("--whole-chain")
        .arg("--json")
        .assert()
        .success();

    // This call doesn't specify any permissions, so the will be removed.
    Command::cargo_bin("ya-provider")
        .unwrap()
        .args(["--cert-dir", cert_dir.as_path().to_str().unwrap()])
        .arg("keystore")
        .arg("remove")
        .arg("2e6e701d")
        .arg("--json")
        .assert()
        .success();

    let result = list_certificates_command(&cert_dir).unwrap();

    assert_eq!(check_permissions(&result, "4e0df976"), "outbound-manifest");
    assert_eq!(check_permissions(&result, "c128af8c"), "outbound-manifest");
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

fn check_permissions(certs: &HashMap<String, Value>, id: &str) -> String {
    certs[id]
        .get("Permissions")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string()
}
