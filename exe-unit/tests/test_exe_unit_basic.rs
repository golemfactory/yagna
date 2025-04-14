use test_context::test_context;

use ya_client_model::activity::ExeScriptCommand;
use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::file::generate_image;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::server_external::start_http;
use ya_framework_basic::test_dirs::cargo_binary;
use ya_framework_basic::{resource, temp_dir};
use ya_mock_runtime::testing::{create_exe_unit, exe_unit_config, ExeUnitExt};

#[cfg_attr(not(feature = "system-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_exe_unit_start_terminate(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("exe-unit-start-terminate")?;
    let temp_dir = dir.path();
    let image_repo = temp_dir.join("images");

    generate_image(&image_repo, "image-1", 4096_usize, 10);
    start_http(ctx, image_repo)
        .await
        .expect("unable to start http servers");

    let config = exe_unit_config(
        temp_dir,
        &resource!("agreement.json"),
        cargo_binary("ya-mock-runtime")?,
    );

    let exe = create_exe_unit(config.clone(), ctx).await.unwrap();
    exe.await_init().await.unwrap();

    log::info!("Sending [deploy, start] batch for execution.");

    exe.wait_for_batch(&exe.deploy(None).await.unwrap())
        .await
        .unwrap();
    exe.wait_for_batch(&exe.start(vec![]).await.unwrap())
        .await
        .unwrap();

    log::info!("Sending shutdown request.");

    exe.exec(None, vec![ExeScriptCommand::Terminate {}])
        .await
        .unwrap();

    exe.shutdown().await.unwrap();
    Ok(())
}
