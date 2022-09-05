#[macro_use]
extern crate serial_test;

use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use serde_json::{json, Value};
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
    r#"[{ 
        "ID": "5ce448a6", 
        "Pattern": "domain.com", 
        "Type": "strict" 
    }]"#;
    "Add single domain pattern"
)]
#[serial]
fn whitelist_add_test(add: &[&str], pattern_type: &str, expected_add_json_out: &str) {
    clean_data_dir();
    let mut cmd = Command::cargo_bin("ya-provider").unwrap();
    let expected_add_json_out: Value = serde_json::from_str(&expected_add_json_out).unwrap();
    let expected_add_json_out = serde_json::to_string_pretty(&expected_add_json_out).unwrap();
    let expected_add_json_out = format!("{expected_add_json_out}\n");
    cmd.arg("whitelist")
        .arg("add")
        .arg("-p")
        .args(add)
        .arg("-t")
        .arg(pattern_type.to_string())
        .arg("--json")
        .args(data_dir_args())
        .assert()
        .stdout(expected_add_json_out.to_string())
        .success();
}

fn clean_data_dir() {
    let data_dir = data_dir_path();
    fs::remove_dir_all(&data_dir).expect("Can delete data dir");
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
