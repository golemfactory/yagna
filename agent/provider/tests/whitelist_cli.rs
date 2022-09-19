#[macro_use]
extern crate serial_test;

use std::fs;
use std::path::PathBuf;

use assert_cmd::{assert::Assert, Command};
use serde_json::Value;
use test_case::test_case;

#[serial]
#[test]
fn empty_whitelist_json() {
    clean_data_dir();
    Command::cargo_bin("ya-provider")
        .unwrap()
        .arg("whitelist")
        .arg("list")
        .arg("--json")
        .args(data_dir_args())
        .assert()
        .stdout("[]\n")
        .success();
}

#[test_case(
    &["domain.com"],  
    "strict",
    r#"[{ "ID": "5ce448a6", "Pattern": "domain.com", "Type": "strict" }]"#;
    "Adding one pattern"
)]
#[test_case(
    &["dom.*\\.com", "myapp.com", "other\\.*"],  
    "regex",
    r#"[
        { "ID": "979b6b99", "Pattern": "dom.*\\.com", "Type": "regex" },
        { "ID": "89dcf5f6", "Pattern": "myapp.com", "Type": "regex" },
        { "ID": "c31deaea", "Pattern": "other\\.*", "Type": "regex" }
    ]"#;
    "Adding multiple patterns"
)]
#[test_case(
    &["domain.com", "domain.com"],  
    "strict",
    r#"[
        { "ID": "5ce448a6", "Pattern": "domain.com", "Type": "strict" }
    ]"#;
    "Adding duplicated patterns results in one result"
)]
#[serial]
fn whitelist_add_test(add: &[&str], pattern_type: &str, expected_add_json_out: &str) {
    clean_data_dir();
    let expected_add_json_out = json_to_printed_output(expected_add_json_out);
    whitelist_add(add, pattern_type)
        .stdout(expected_add_json_out.to_string())
        .success();
}

#[test_case(
    &["domain.com"],  
    "strict",
    &["5ce448a6"],
    r#"[{ "ID": "5ce448a6", "Pattern": "domain.com", "Type": "strict" }]"#,
    "[]";
    "Adding one pattern, removing it, empty list."
)]
#[test_case(
    &["domain.com"],  
    "strict",
    &["5ce448a6", "5ce448a6"],
    r#"[{ "ID": "5ce448a6", "Pattern": "domain.com", "Type": "strict" }]"#,
    "[]";
    "Adding one pattern, removing with duplicated ids, empty list."
)]
#[test_case(
    &["domain.com", "another.com"],  
    "strict",
    &["5ce448a6"],
    r#"[{ "ID": "5ce448a6", "Pattern": "domain.com", "Type": "strict" }]"#,
    r#"[ { "ID": "ee0cc088",  "Pattern": "another.com",  "Type": "strict" }]"#;
    "Adding two patterns, removing one, one on the list."
)]
#[test_case(
    &["domain.com"],  
    "strict",
    &["no_such_id"],
    "[]",
    r#"[{ "ID": "5ce448a6", "Pattern": "domain.com", "Type": "strict" }]"#;
    "Adding one pattern, removing not existing, one on the list."
)]
#[test_case(
    &[],
    "",
    &["no_such_id", "nope"],
    "[]",
    "[]";
    "Removing on empty list. List on empty produces empty list."
)]
#[serial]
fn whitelist_remove_test(
    add: &[&str],
    pattern_type: &str,
    remove_ids: &[&str],
    remove_out: &str,
    list_out: &str,
) {
    clean_data_dir();
    if !add.is_empty() {
        whitelist_add(add, pattern_type).success();
    }
    let remove_out = json_to_printed_output(remove_out);
    whitelist_remove(remove_ids).stdout(remove_out).success();
    let list_out = json_to_printed_output(list_out);
    whitelist_list().stdout(list_out).success();
}

// Whitelist test utils

fn whitelist_add(add: &[&str], pattern_type: &str) -> Assert {
    let mut cmd = Command::cargo_bin("ya-provider").unwrap();
    cmd.arg("whitelist")
        .arg("add")
        .arg("-p")
        .args(add)
        .arg("-t")
        .arg(pattern_type.to_string())
        .arg("--json")
        .args(data_dir_args())
        .assert()
}

fn whitelist_remove(ids: &[&str]) -> Assert {
    let mut cmd = Command::cargo_bin("ya-provider").unwrap();
    cmd.arg("whitelist")
        .arg("remove")
        .args(ids)
        .arg("--json")
        .args(data_dir_args())
        .assert()
}

fn whitelist_list() -> Assert {
    let mut cmd = Command::cargo_bin("ya-provider").unwrap();
    cmd.arg("whitelist")
        .arg("list")
        .arg("--json")
        .args(data_dir_args())
        .assert()
}

fn json_to_printed_output(out: &str) -> String {
    let out: Value = serde_json::from_str(&out).unwrap();
    let out = serde_json::to_string_pretty(&out).unwrap();
    format!("{out}\n")
}

fn clean_data_dir() {
    let data_dir = data_dir_path();
    if data_dir.exists() {
        fs::remove_dir_all(&data_dir).expect("Can delete data dir");
    }
    // Without creating dir eprintln breaks tests https://github.com/golemfactory/yagna/blob/master/utils/path/src/data_dir.rs#L23
    fs::create_dir(data_dir).unwrap();
}

fn data_dir_args() -> [String; 2] {
    let data_dir = data_dir_path();
    let data_dir = data_dir.to_str().unwrap().to_string();
    ["--data-dir".to_string(), data_dir]
}

fn data_dir_path() -> PathBuf {
    let mut data_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    data_dir.push("data_dir");
    data_dir
}
