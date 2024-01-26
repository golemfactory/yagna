use test_context::test_context;

use ya_client_model::activity::ExeScriptCommand;
use ya_exe_unit::message::{Shutdown, ShutdownReason};
use ya_exe_unit::send_script;
use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::file::generate_image;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::server_external::start_http;
use ya_framework_basic::test_dirs::cargo_binary;
use ya_framework_basic::{resource, temp_dir};
use ya_mock_runtime::testing::{create_exe_unit, exe_unit_config, ExeUnitExt};

#[cfg_attr(not(feature = "framework-test"), ignore)]
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
    let mut finish = exe.finish_notifier().await?;

    log::info!("Sending [deploy, start] batch for execution.");

    let batch_id = send_script(
        exe.addr.clone(),
        config.service_id.clone(),
        vec![
            ExeScriptCommand::Deploy {
                net: vec![],
                progress: None,
                env: Default::default(),
                hosts: Default::default(),
                hostname: None,
                volumes: vec![],
            },
            ExeScriptCommand::Start { args: vec![] },
        ],
    )
    .await
    .unwrap();

    exe.wait_for_batch(&batch_id).await.unwrap();

    log::info!("Sending shutdown request.");

    exe.exec(None, vec![ExeScriptCommand::Terminate {}])
        .await
        .unwrap();
    exe.addr.send(Shutdown(ShutdownReason::Finished)).await.ok();

    log::info!("Waiting for shutdown..");

    finish.recv().await.unwrap();
    Ok(())
}
