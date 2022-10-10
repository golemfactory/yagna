use std::path::PathBuf;

use assert_cmd::Command;
use serial_test::serial;

#[serial]
#[test]
fn keystore_list_cmd_creates_cert_dir_in_data_dir_set_by_env() {
    // Having
    let mut data_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    data_dir.push("data_dir");
    let mut cert_dir = data_dir.clone();
    cert_dir.push("cert_dir");
    #[allow(unused_must_use)]
    {
        std::fs::remove_dir_all(&data_dir); // ignores error if does not exist
        std::fs::remove_dir_all(&cert_dir); // ignores error if does not exist
    }
    // When
    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.as_path().to_str().unwrap())
        .arg("keystore")
        .arg("list")
        .arg("--json")
        .assert()
        .stdout("[]\n")
        .success();
    // Then
    assert!(
        cert_dir.exists(),
        "Cert dir has been created inside of data dir"
    );
}

#[serial]
#[test]
fn keystore_list_cmd_creates_cert_dir_in_dir_set_by_env() {
    // Having
    let mut data_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    data_dir.push("data_dir");
    let mut cert_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    cert_dir.push("cert_dir");
    #[allow(unused_must_use)]
    {
        std::fs::remove_dir_all(&data_dir); // ignores error if does not exist
        std::fs::remove_dir_all(&cert_dir); // ignores error if does not exist
    }
    let mut wrong_cert_dir = data_dir.clone();
    wrong_cert_dir.push("cert_dir");
    // When
    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.as_path().to_str().unwrap())
        .env("PROVIDER_CERT_DIR", cert_dir.as_path().to_str().unwrap())
        .arg("keystore")
        .arg("list")
        .arg("--json")
        .assert()
        .stdout("[]\n")
        .success();
    // Then
    assert!(
        cert_dir.exists(),
        "Cert dir has been created inside of dir pointed by $PROVIDER_CERT_DIR"
    );
    assert!(
        !wrong_cert_dir.exists(),
        "Cert dir has not been created inside of data dir pointed by $DATA_DIR"
    );
}

#[serial]
#[test]
fn keystore_list_cmd_creates_cert_dir_in_dir_set_by_arg() {
    // Having
    let mut data_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    data_dir.push("data_dir");
    let mut cert_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    cert_dir.push("cert_dir");
    #[allow(unused_must_use)]
    {
        std::fs::remove_dir_all(&data_dir); // ignores error if does not exist
        std::fs::remove_dir_all(&cert_dir); // ignores error if does not exist
    }
    // When
    Command::cargo_bin("ya-provider")
        .unwrap()
        .env("DATA_DIR", data_dir.as_path().to_str().unwrap())
        .args(["--cert-dir", cert_dir.as_path().to_str().unwrap()])
        .arg("keystore")
        .arg("list")
        .arg("--json")
        .assert()
        .stdout("[]\n")
        .success();
    // Then
    assert!(
        cert_dir.exists(),
        "Cert dir has been created inside of dir pointed by --cert-dir arg"
    );
}
