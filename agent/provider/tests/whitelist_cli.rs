#![allow(clippy::items_after_test_module)]

use assert_cmd::Command;
use pretty_assertions::assert_eq;
use serde_json::{json, Value};
use tempdir::TempDir;
use test_case::test_case;

#[test]
fn empty_whitelist_json() {
    let data_dir = prepare_data_dir();

    Command::cargo_bin("ya-provider")
        .unwrap()
        .arg("whitelist")
        .arg("list")
        .arg("--json")
        .args(data_dir_args(data_dir.path().to_str().unwrap()))
        .assert()
        .stdout("[]\n")
        .success();
}

#[test_case(
    &["domain.com"],  
    "strict",
    json!([{ "ID": "5ce448a6", "Pattern": "domain.com", "Type": "strict" }]);
    "Adding one pattern"
)]
#[test_case(
    &["dom.*\\.com", "myapp.com", "other\\.*"],  
    "regex",
    json!([
        { "ID": "979b6b99", "Pattern": "dom.*\\.com", "Type": "regex" },
        { "ID": "89dcf5f6", "Pattern": "myapp.com", "Type": "regex" },
        { "ID": "c31deaea", "Pattern": "other\\.*", "Type": "regex" }
    ]);
    "Adding multiple patterns"
)]
#[test_case(
    &["domain.com", "domain.com"],  
    "strict",
    json!([
        { "ID": "5ce448a6", "Pattern": "domain.com", "Type": "strict" }
    ]);
    "Adding duplicated patterns results in one result"
)]
fn whitelist_add_test(add: &[&str], pattern_type: &str, expected_add_json_out: serde_json::Value) {
    let data_dir = prepare_data_dir();

    let output = whitelist_add(add, pattern_type, data_dir.path().to_str().unwrap());

    assert_eq!(output, expected_add_json_out);
}

#[test_case(
    &["domain.com"],  
    "strict",
    &["5ce448a6"],
    json!([{ "ID": "5ce448a6", "Pattern": "domain.com", "Type": "strict" }]),
    json!([]);
    "Adding one pattern, removing it, empty list."
)]
#[test_case(
    &["domain.com"],  
    "strict",
    &["5ce448a6", "5ce448a6"],
    json!([{ "ID": "5ce448a6", "Pattern": "domain.com", "Type": "strict" }]),
    json!([]);
    "Adding one pattern, removing with duplicated ids, empty list."
)]
#[test_case(
    &["domain.com", "another.com"],  
    "strict",
    &["5ce448a6"],
    json!([{ "ID": "5ce448a6", "Pattern": "domain.com", "Type": "strict" }]),
    json!([ { "ID": "ee0cc088",  "Pattern": "another.com",  "Type": "strict" }]);
    "Adding two patterns, removing one, one on the list."
)]
#[test_case(
    &["domain.com"],  
    "strict",
    &["no_such_id"],
    json!([]),
    json!([{ "ID": "5ce448a6", "Pattern": "domain.com", "Type": "strict" }]);
    "Adding one pattern, removing not existing, one on the list."
)]
#[test_case(
    &[],
    "",
    &["no_such_id", "nope"],
    json!([]),
    json!([]);
    "Removing on empty list. List on empty produces empty list."
)]
fn whitelist_remove_test(
    add: &[&str],
    pattern_type: &str,
    remove_ids: &[&str],
    remove_out: Value,
    list_out: Value,
) {
    let data_dir = prepare_data_dir();
    let data_dir_path = data_dir.path().to_str().unwrap();

    if !add.is_empty() {
        let _ = whitelist_add(add, pattern_type, data_dir_path);
    }

    assert_eq!(whitelist_remove(remove_ids, data_dir_path), remove_out);

    assert_eq!(whitelist_list(data_dir_path), list_out);
}

// Whitelist test utils

fn whitelist_add(add: &[&str], pattern_type: &str, data_dir: &str) -> Value {
    let output = Command::cargo_bin("ya-provider")
        .unwrap()
        .arg("whitelist")
        .arg("add")
        .arg("-p")
        .args(add)
        .arg("-t")
        .arg(pattern_type)
        .arg("--json")
        .args(data_dir_args(data_dir))
        .output()
        .unwrap();

    assert!(output.status.success());

    serde_json::from_slice(&output.stdout).unwrap()
}

fn whitelist_remove(ids: &[&str], data_dir: &str) -> Value {
    let output = Command::cargo_bin("ya-provider")
        .unwrap()
        .arg("whitelist")
        .arg("remove")
        .args(ids)
        .arg("--json")
        .args(data_dir_args(data_dir))
        .output()
        .unwrap();

    assert!(output.status.success());

    serde_json::from_slice(&output.stdout).unwrap()
}

fn whitelist_list(data_dir: &str) -> Value {
    let output = Command::cargo_bin("ya-provider")
        .unwrap()
        .arg("whitelist")
        .arg("list")
        .arg("--json")
        .args(data_dir_args(data_dir))
        .output()
        .unwrap();

    assert!(output.status.success());

    serde_json::from_slice(&output.stdout).unwrap()
}

fn prepare_data_dir() -> TempDir {
    TempDir::new("whitelist-cli-test-data-dir").unwrap()
}

fn data_dir_args(data_dir: &str) -> [String; 2] {
    ["--data-dir".to_string(), data_dir.to_owned()]
}
