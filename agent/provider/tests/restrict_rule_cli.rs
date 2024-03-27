mod utils;

use assert_cmd::Command;
use serde_json::json;
use test_case::test_case;

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
    let cert_id = fingerprint(&certs.join("root-certificate.signed.json")).unwrap();

    pretty_assertions::assert_eq!(
        result[rule]["certified"]
            .as_array()
            .unwrap()
            .contains(&json!(cert_id)),
        true
    );

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.path().to_str().unwrap())
        .args(format!("rule remove {rule} certified cert-id {cert_id}").split(' '))
        .assert()
        .success();

    let result = list_rules_command(data_dir.path());
    pretty_assertions::assert_eq!(
        result[rule]["certified"]
            .as_array()
            .unwrap()
            .contains(&json!(cert_id)),
        false
    );
}
