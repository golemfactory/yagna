#![allow(clippy::items_after_test_module)]
mod utils;

use assert_cmd::Command;
use pretty_assertions::assert_eq;
use serde_json::{json, Value};
use std::{collections::HashSet, iter::FromIterator, path::PathBuf, str::from_utf8};
use tempdir::TempDir;
use test_case::test_case;

use crate::utils::rules::cli::{
    add_certificate_to_keystore, list_certs, list_rules_command, remove_certificate_from_keystore,
    rule_to_mode,
};

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
            "everyone": "whitelist",
            "audited-payload": {},
            "partner": {}
          },
          "blacklist": {
            "enabled": false,
            "identity": [],
            "certified": []
          },
          "allow-only": {
            "enabled": false,
            "identity": [],
            "certified": []
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

#[test_case("partner")]
#[test_case("audited-payload")]
fn adding_rule_for_non_existing_certificate_should_fail(rule: &str) {
    let data_dir = prepare_test_dir();

    let cert_id = "deadbeef";

    let stderr = Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .env("RUST_LOG", "info") // tests asserting error may also print logs
        .arg("rule")
        .arg("set")
        .arg("outbound")
        .arg(rule)
        .arg("cert-id")
        .arg(cert_id)
        .arg("--mode")
        .arg("all")
        .output()
        .unwrap()
        .stderr;

    let stderr = from_utf8(&stderr).unwrap();
    let expected = format!("No cert id: {cert_id} found in keystore");

    assert!(stderr.contains(&expected));
}

#[test_case("partner", "all")]
#[test_case("partner", "none")]
#[test_case("partner", "whitelist")]
#[serial_test::serial]
fn rule_set_should_fail_on_unsupported_certificate(rule: &str, mode: &str) {
    let (data_dir, resource_cert_dir) = prepare_test_dir_with_cert_resources();

    let cert_id =
        add_certificate_to_keystore(data_dir.path(), &resource_cert_dir, "foo_req.cert.pem");

    let stderr = Command::cargo_bin("ya-provider")
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
        .output()
        .unwrap()
        .stderr;

    let stderr = from_utf8(&stderr).unwrap();
    let expected = regex::Regex::new("Error: Failed to set partner mode for certificate 25b9430c. .* mode can be set only for Golem certificate.\n").unwrap();

    assert!(expected.is_match(stderr));
}

#[test_case("audited-payload", "all")]
#[test_case("audited-payload", "none")]
#[test_case("audited-payload", "whitelist")]
#[serial_test::serial]
fn rule_set_should_edit_x509_certificate_rules(rule: &str, mode: &str) {
    rule_set_should_edit_certificate_rules(rule, mode, "foo_req.cert.pem")
}

#[test_case("partner", "all")]
#[test_case("partner", "none")]
#[test_case("partner", "whitelist")]
#[serial_test::serial]
fn rule_set_should_edit_golem_certificate_rules(rule: &str, mode: &str) {
    rule_set_should_edit_certificate_rules(rule, mode, "partner-certificate.signed.json")
}

fn rule_set_should_edit_certificate_rules(rule: &str, mode: &str, cert: &str) {
    let (data_dir, resource_cert_dir) = prepare_test_dir_with_cert_resources();

    let cert_id = add_certificate_to_keystore(data_dir.path(), &resource_cert_dir, cert);

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
fn rule_set_with_import_golem_cert_should_add_cert_to_keystore_and_to_rulestore(
    rule: &str,
    mode: &str,
) {
    let (data_dir, resource_cert_dir) = prepare_test_dir_with_cert_resources();

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .arg("rule")
        .arg("set")
        .arg("outbound")
        .arg(rule)
        .arg("import-cert")
        .arg(resource_cert_dir.join("partner-certificate.signed.json"))
        .arg("--mode")
        .arg(mode)
        .assert()
        .success();

    let result = list_rules_command(data_dir.path());
    let added_certs = list_certs(data_dir.path());

    assert!(!added_certs.is_empty());
    for cert in added_certs {
        let mode_actual = result["outbound"][rule]
            .as_object()
            .and_then(|obj| obj.iter().find(|(id, _cert)| id.starts_with(&cert)))
            .map(|(_id, value)| &value["mode"]);

        assert_eq!(mode_actual.unwrap(), mode);
    }
}

#[test_case("audited-payload", "all")]
#[test_case("audited-payload", "none")]
#[test_case("audited-payload", "whitelist")]
#[serial_test::serial]
fn rule_set_with_import_x509_cert_chain_should_add_whole_to_keystore_and_leaf_to_rulestore(
    rule: &str,
    mode: &str,
) {
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

    let rules_list = list_rules_command(data_dir.path());
    let added_certs = list_certs(data_dir.path());
    let added_certs: HashSet<String> = HashSet::from_iter(added_certs);

    let leaf_cert_id = added_certs.get("55e451bd").unwrap();
    let leaf_mode = get_rule_mode(&rules_list, rule, leaf_cert_id);
    assert_eq!(leaf_mode.unwrap(), mode);

    let root_cert_id = added_certs.get("fe4f04e2").unwrap();
    let root_mode = get_rule_mode(&rules_list, rule, root_cert_id);
    assert_eq!(root_mode, None);
}

fn get_rule_mode<'a>(rules_list: &'a Value, rule: &'a str, cert_id: &'a str) -> Option<&'a Value> {
    rules_list["outbound"][rule]
        .as_object()
        .and_then(|obj| obj.iter().find(|(id, _cert)| id.starts_with(cert_id)))
        .map(|(_id, value)| &value["mode"])
}

#[test_case("audited-payload", "foo_ca.cert.pem")]
#[test_case("partner", "partner-certificate.signed.json")]
#[serial_test::serial]
fn removing_cert_should_also_remove_its_rule(rule: &str, cert: &str) {
    let (data_dir, resource_cert_dir) = prepare_test_dir_with_cert_resources();

    let cert_id = add_certificate_to_keystore(data_dir.path(), &resource_cert_dir, cert);

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
