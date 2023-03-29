use std::path::{Path, PathBuf};

use assert_cmd::Command;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempdir::TempDir;
use test_case::test_case;

static INIT: std::sync::Once = std::sync::Once::new();

#[test]
fn rule_list_cmd_should_print_default_rules() {
    let data_dir = prepare_test_dir();

    let output = Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .arg("rule")
        .arg("list")
        .arg("--json")
        .output()
        .unwrap();

    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(
        result,
        json!({
          "outbound": {
            "enabled": true,
            "everyone": "none",
            "audited-payload": {
              "default": {
                "mode": "all",
                "description": "Default setting"
              }
            },
            "partner": {}
          }
        })
    );
}

#[test]
fn rule_set_should_disable_and_enable_feature() {
    let data_dir = prepare_test_dir();

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .arg("rule")
        .arg("set")
        .arg("outbound")
        .arg("disable")
        .assert()
        .success();

    let result = list_rules_command(data_dir.path());

    assert_eq!(&result["outbound"]["enabled"], false);

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .arg("rule")
        .arg("set")
        .arg("outbound")
        .arg("enable")
        .assert()
        .success();

    let result = list_rules_command(data_dir.path());

    assert_eq!(&result["outbound"]["enabled"], true);
}

#[test_case("all")]
#[test_case("none")]
#[test_case("whitelist")]
fn rule_set_should_edit_everyone_mode(mode: &str) {
    let rule = "everyone";
    let data_dir = prepare_test_dir();

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .arg("rule")
        .arg("set")
        .arg("outbound")
        .arg(rule)
        .arg("--mode")
        .arg(mode)
        .assert()
        .success();

    let result = list_rules_command(data_dir.path());

    assert_eq!(&result["outbound"][rule], mode);
}

#[test_case("audited-payload", "all")]
#[test_case("audited-payload", "none")]
#[test_case("audited-payload", "whitelist")]
fn rule_set_should_edit_default_modes_for_certificate_rules(rule: &str, mode: &str) {
    let data_dir = prepare_test_dir();

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .arg("rule")
        .arg("set")
        .arg("outbound")
        .arg(rule)
        .arg("--mode")
        .arg(mode)
        .assert()
        .success();

    let result = list_rules_command(data_dir.path());

    assert_eq!(&result["outbound"][rule]["default"]["mode"], mode);
    assert_eq!(
        &result["outbound"][rule]["default"]["description"],
        "Default setting"
    );
}

#[test_case("partner")]
fn adding_rule_for_non_existing_certificate_should_fail(rule: &str) {
    let data_dir = prepare_test_dir();

    let cert_id = "deadbeef";

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .arg("rule")
        .arg("set")
        .arg("outbound")
        .arg(rule)
        .arg("cert-id")
        .arg(cert_id)
        .arg("--mode")
        .arg("all")
        .assert()
        .stderr(format!(
            "Error: Setting Partner mode all failed: No cert id: {cert_id} found in keystore\n"
        ));
}

#[test_case("partner", "all")]
#[test_case("partner", "none")]
#[test_case("partner", "whitelist")]
#[serial_test::serial]
fn rule_set_should_edit_certificate_rules(rule: &str, mode: &str) {
    let (data_dir, resource_cert_dir) = prepare_test_dir_with_cert_resources();

    let cert_id = add_certificate_to_keystore(data_dir.path(), &resource_cert_dir);

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .arg("rule")
        .arg("set")
        .arg("outbound")
        .arg(rule)
        .arg("cert-id")
        .arg(&cert_id)
        .arg("--mode")
        .arg(mode)
        .assert()
        .success();

    let result = list_rules_command(data_dir.path());
    let mode_actual = rule_to_mode(&result["outbound"][rule], &cert_id);

    assert_eq!(mode_actual.unwrap(), mode);
}

#[test_case("partner", "all")]
#[test_case("partner", "none")]
#[test_case("partner", "whitelist")]
#[serial_test::serial]
fn rule_set_with_import_cert_should_add_to_keystore_and_rulestore(rule: &str, mode: &str) {
    let (data_dir, resource_cert_dir) = prepare_test_dir_with_cert_resources();

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .arg("rule")
        .arg("set")
        .arg("outbound")
        .arg(rule)
        .arg("import-cert")
        .arg(resource_cert_dir.join("foo_ca-chain.cert.pem"))
        .arg("--mode")
        .arg(mode)
        .assert()
        .success();

    let result = list_rules_command(data_dir.path());
    let added_certs = list_certs(data_dir.path());

    for cert in added_certs {
        let mode_actual = rule_to_mode(&result["outbound"][rule], &cert);
        assert_eq!(mode_actual.unwrap(), mode);
    }
}

#[test]
#[serial_test::serial]
fn removing_cert_should_also_remove_its_rule() {
    let (data_dir, resource_cert_dir) = prepare_test_dir_with_cert_resources();

    let rule = "partner";

    let cert_id = add_certificate_to_keystore(data_dir.path(), &resource_cert_dir);

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .arg("rule")
        .arg("set")
        .arg("outbound")
        .arg(rule)
        .arg("cert-id")
        .arg(&cert_id)
        .arg("--mode")
        .arg("all")
        .assert()
        .success();

    remove_certificate_from_keystore(data_dir.path(), &cert_id);

    let result = list_rules_command(data_dir.path());

    assert_eq!(result["outbound"][rule][&cert_id], serde_json::Value::Null);
}

fn list_rules_command(data_dir: &Path) -> serde_json::Value {
    let output = Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.to_str().unwrap())
        .arg("rule")
        .arg("list")
        .arg("--json")
        .output()
        .unwrap();

    serde_json::from_slice(&output.stdout).unwrap()
}

fn rule_to_mode<'json, 's>(
    rule: &'json serde_json::Value,
    cert_prefix: &'s str,
) -> Option<&'json serde_json::Value> {
    rule.as_object()
        .and_then(|obj| obj.iter().find(|(id, _cert)| id.starts_with(&cert_prefix)))
        .map(|(_id, value)| &value["mode"])
}

fn remove_certificate_from_keystore(data_dir: &Path, cert_id: &str) {
    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.to_str().unwrap())
        .arg("keystore")
        .arg("remove")
        .arg(cert_id)
        .assert()
        .success();
}

fn add_certificate_to_keystore(data_dir: &Path, resource_cert_dir: &Path) -> String {
    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.to_str().unwrap())
        .arg("keystore")
        .arg("add")
        .arg(resource_cert_dir.join("foo_ca-chain.cert.pem"))
        .assert()
        .success();

    list_certs(data_dir)[0].clone()
}

fn list_certs(data_dir: &Path) -> Vec<String> {
    let output = Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.to_str().unwrap())
        .arg("keystore")
        .arg("list")
        .arg("--json")
        .output()
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    result
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["ID"].as_str().unwrap().to_string())
        .collect()
}

fn prepare_test_dir() -> TempDir {
    TempDir::new("rule-cli-test-data-dir").unwrap()
}

fn prepare_test_dir_with_cert_resources() -> (TempDir, PathBuf) {
    let test_dir = TempDir::new("rule-cli-test-data-dir").unwrap();

    let cert_resources_dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));

    INIT.call_once(|| {
        if cert_resources_dir.exists() {
            std::fs::remove_dir_all(&cert_resources_dir)
                .expect("Can delete test cert resources dir");
        }
        std::fs::create_dir_all(&cert_resources_dir).expect("Can create temp dir");
        ya_manifest_test_utils::TestResources::unpack_cert_resources(&cert_resources_dir);
    });

    (test_dir, cert_resources_dir)
}
