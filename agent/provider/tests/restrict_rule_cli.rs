mod utils;

use assert_cmd::Command;
use serde_json::json;
use std::str::FromStr;
use test_case::test_case;
use ya_client_model::NodeId;

use crate::utils::rules::cli::list_rules_command;
use crate::utils::rules::{fingerprint, init_certificates};

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
