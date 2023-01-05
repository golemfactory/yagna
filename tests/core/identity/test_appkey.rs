use serial_test;

use ya_test_framework::framework::macros::prepare_test_dir;
use ya_test_framework::YagnaMock;

#[cfg_attr(not(feature = "framework-test"), ignore)]
#[serial_test::serial]
async fn test_appkey_removal() {
    let yagna = YagnaMock::new(&prepare_test_dir!())
        .unwrap()
        .service_run()
        .await
        .unwrap();

    yagna
        .command()
        .arg("app-key")
        .arg("create")
        .arg("test-appkey")
        .assert()
        .success();

    yagna
        .command()
        .arg("app-key")
        .arg("drop")
        .arg("test-appkey")
        .assert()
        .success();
}
