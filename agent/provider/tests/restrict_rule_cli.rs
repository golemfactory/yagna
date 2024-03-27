mod utils;

use assert_cmd::Command;
use convert_case::{Case, Casing};
use serde_json::json;
use std::str::FromStr;
use test_case::test_case;

use crate::utils::rules::cli::{list_rules_command, remove_certificate_from_keystore};
use crate::utils::rules::{fingerprint, init_certificates};

use ya_client_model::NodeId;
use ya_framework_basic::temp_dir;

#[test_case("blacklist")]
#[test_case("allow-only")]
#[serial_test::serial]
fn restrict_rule_add_remove_certified_rules(rule: &str) {
    let certs = init_certificates();
    let data_dir = temp_dir!("restrict_rule_add_remove_certified_rules").unwrap();

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .args(format!("rule add {rule} certified import-cert").split(' '))
        .arg(certs.join("root-certificate.signed.json"))
        .assert()
        .success();

    let result = list_rules_command(data_dir.path());
    let cert_id1 = fingerprint(&certs.join("root-certificate.signed.json")).unwrap();

    pretty_assertions::assert_eq!(
        result[rule]["certified"]
            .as_array()
            .unwrap()
            .contains(&json!(cert_id1)),
        true
    );

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .args(format!("rule add {rule} certified import-cert").split(' '))
        .arg(certs.join("partner-certificate.signed.json"))
        .assert()
        .success();

    let result = list_rules_command(data_dir.path());
    let cert_id2 = fingerprint(&certs.join("partner-certificate.signed.json")).unwrap();

    pretty_assertions::assert_eq!(
        result[rule]["certified"]
            .as_array()
            .unwrap()
            .contains(&json!(cert_id2)),
        true
    );

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .args(format!("rule remove {rule} certified cert-id {cert_id1}").split(' '))
        .assert()
        .success();

    let result = list_rules_command(data_dir.path());
    pretty_assertions::assert_eq!(
        result[rule]["certified"]
            .as_array()
            .unwrap()
            .contains(&json!(cert_id1)),
        false
    );
    pretty_assertions::assert_eq!(
        result[rule]["certified"]
            .as_array()
            .unwrap()
            .contains(&json!(cert_id2)),
        true
    );
}

#[test_case("blacklist")]
#[test_case("allow-only")]
#[serial_test::serial]
fn restrict_rule_add_remove_identity_rules(rule: &str) {
    let data_dir = temp_dir!("restrict_rule_add_remove_identity_rules").unwrap();

    let node1 = NodeId::from_str("0x0000000000000000000000000000000000000000").unwrap();
    let node2 = NodeId::from_str("0x0000000000000000000000000000000000000001").unwrap();

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .args(format!("rule add {rule} by-node-id --address {node1}").split(' '))
        .assert()
        .success();

    let result = list_rules_command(data_dir.path());

    pretty_assertions::assert_eq!(
        result[rule]["identity"]
            .as_array()
            .unwrap()
            .contains(&json!(node1)),
        true
    );

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .args(format!("rule add {rule} by-node-id --address {node2}").split(' '))
        .assert()
        .success();

    let result = list_rules_command(data_dir.path());

    pretty_assertions::assert_eq!(
        result[rule]["identity"]
            .as_array()
            .unwrap()
            .contains(&json!(node2)),
        true
    );

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .args(format!("rule remove {rule} by-node-id --address {node1}").split(' '))
        .assert()
        .success();

    let result = list_rules_command(data_dir.path());
    pretty_assertions::assert_eq!(
        result[rule]["identity"]
            .as_array()
            .unwrap()
            .contains(&json!(node1)),
        false
    );
    pretty_assertions::assert_eq!(
        result[rule]["identity"]
            .as_array()
            .unwrap()
            .contains(&json!(node2)),
        true
    );
}

#[test_case("blacklist")]
#[test_case("allow-only")]
#[serial_test::serial]
fn restrict_rule_removing_cert_should_also_remove_its_rule(rule: &str) {
    let certs = init_certificates();
    let data_dir = temp_dir!("restrict_rule_removing_cert_should_also_remove_its_rule").unwrap();

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .args(format!("rule add {rule} certified import-cert").split(' '))
        .arg(certs.join("partner-certificate.signed.json"))
        .assert()
        .success();

    let cert_id = fingerprint(&certs.join("partner-certificate.signed.json")).unwrap();
    remove_certificate_from_keystore(data_dir.path(), &cert_id);

    let result = list_rules_command(data_dir.path());
    pretty_assertions::assert_eq!(
        result[rule]["certified"]
            .as_array()
            .unwrap()
            .contains(&json!(cert_id)),
        false
    );
}

#[test_case("blacklist")]
#[test_case("allow-only")]
#[serial_test::serial]
fn restrict_rule_should_showup_in_keystore_cli(rule: &str) {
    let certs = init_certificates();
    let data_dir = temp_dir!("restrict_rule_should_showup_in_keystore_cli").unwrap();

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .args(format!("rule add {rule} certified import-cert").split(' '))
        .arg(certs.join("partner-certificate.signed.json"))
        .assert()
        .success();

    let cert_id = fingerprint(&certs.join("partner-certificate.signed.json")).unwrap();

    let output = Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .arg("keystore")
        .arg("list")
        .arg("--json")
        .output()
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    pretty_assertions::assert_eq!(
        result[0]["Rules"].as_str().unwrap(),
        rule.to_case(Case::UpperCamel)
    );
    pretty_assertions::assert_eq!(cert_id.starts_with(result[0]["ID"].as_str().unwrap()), true);
}
