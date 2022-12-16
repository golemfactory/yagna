use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::{json, Value};
use serial_test::serial;
use test_case::test_case;

fn prepare_test_dir() -> PathBuf {
    let mut data_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    data_dir.push("data-dir");

    let _ = std::fs::remove_dir_all(&data_dir);

    data_dir
}

//TODO Rafał Change serial to tempdir?
//TODO Rafał Remove this test as it doesnt matter if we store it in file or in DB
#[serial]
#[test]
fn rule_list_cmd_should_create_rules_file() {
    let data_dir = prepare_test_dir();

    let rules_file = "rules.json";

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.as_path().to_str().unwrap())
        .arg("rule")
        .arg("list")
        .arg("--json")
        .assert()
        .success();

    let rules_file_path = data_dir.join(PathBuf::from(rules_file));

    assert!(rules_file_path.exists());
}

#[serial]
#[test]
fn rule_list_cmd_should_print_default_rules() {
    let data_dir = prepare_test_dir();

    let output = Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.as_path().to_str().unwrap())
        .arg("rule")
        .arg("list")
        .arg("--json")
        .output()
        .unwrap();

    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    //TODO Rafał Pretty assert
    assert_eq!(
        result,
        json!({
          "outbound": {
            "blocked": false,
            "everyone": "none",
            "audited-payload": {
              "default": {
                "mode": "all",
                "subject": ""
              }
            }
          }
        })
    );
}

#[test_case("all")]
#[test_case("none")]
#[test_case("whitelist")]
#[serial]
fn rule_set_should_edit_everyone_mode(mode: &str) {
    let rule = "everyone";
    let data_dir = prepare_test_dir();

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.as_path().to_str().unwrap())
        .arg("rule")
        .arg("set")
        .arg(rule)
        .arg(mode)
        .assert()
        .success();

    let result = list_rules_command(&data_dir);

    assert_eq!(&result["outbound"][rule], mode);
}

#[test_case("audited-payload", "all")]
#[test_case("audited-payload", "none")]
#[test_case("audited-payload", "whitelist")]
#[serial]
fn rule_set_should_edit_default_modes_for_certificate_rules(rule: &str, mode: &str) {
    let data_dir = prepare_test_dir();

    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.as_path().to_str().unwrap())
        .arg("rule")
        .arg("set")
        .arg(rule)
        .arg(mode)
        .assert()
        .success();

    let result = list_rules_command(&data_dir);

    assert_eq!(&result["outbound"][rule]["default"]["mode"], mode);
    assert_eq!(&result["outbound"][rule]["default"]["subject"], "");
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
