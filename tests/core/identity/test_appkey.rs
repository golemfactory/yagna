use ya_test_framework::framework::macros::{prepare_test_dir, serial_test};
use ya_test_framework::framework::{framework_test, YagnaFramework};

#[cfg_attr(not(feature = "framework-test"), ignore)]
#[framework_test]
async fn test_appkey_removal(framework: YagnaFramework) -> anyhow::Result<()> {
    let yagna = framework.new_node("node1").service_run().await?;

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

    Ok(())
}
