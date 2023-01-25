use std::path::Path;

use assert_cmd::Command;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempdir::TempDir;
use test_case::test_case;

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
            }
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

fn prepare_test_dir() -> TempDir {
    TempDir::new("rule-cli-test-data-dir").unwrap()
}
