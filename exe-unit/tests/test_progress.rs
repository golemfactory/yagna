use anyhow::Context;
use duration_string::DurationString;
use futures::StreamExt;
use std::str::FromStr;
use test_context::test_context;
use url::Url;

use ya_client_model::activity::exe_script_command::ProgressArgs;
use ya_client_model::activity::RuntimeEventKind;
use ya_core_model::activity;
use ya_exe_unit::message::{Shutdown, ShutdownReason};
use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::file::generate_image;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::server_external::start_http;
use ya_framework_basic::test_dirs::{cargo_binary, template};
use ya_framework_basic::{resource, temp_dir};
use ya_mock_runtime::testing::{create_exe_unit, exe_unit_config, ExeUnitExt};

use ya_service_bus::typed as bus;

#[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_progress_reporting(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("progress_reporting")?;
    let temp_dir = dir.path();
    let image_repo = temp_dir.join("images");

    let chunk_size = 4096_usize;
    let chunk_count = 1024 * 1;
    let file_size = (chunk_size * chunk_count) as u64;

    let hash = generate_image(&image_repo, "image-big", chunk_size, chunk_count);
    let package = format!(
        "hash://sha3:{}:http://127.0.0.1:8001/image-big",
        hex::encode(&hash)
    );
    start_http(ctx, image_repo.clone())
        .await
        .expect("unable to start http servers");

    let gsb_url = Url::from_str(&format!("unix://{}/mock-yagna.sock", temp_dir.display())).unwrap();
    std::env::set_var("GSB_URL", gsb_url.to_string());
    ya_sb_router::bind_gsb_router(Some(gsb_url))
        .await
        .context("binding service bus router")?;

    let config = exe_unit_config(
        temp_dir,
        &template(
            &resource!("agreement.template.json"),
            temp_dir.join("agreement.json"),
            &[("task-package", package)],
        )?,
        cargo_binary("ya-mock-runtime")?,
    );

    let exe = create_exe_unit(config.clone(), ctx).await.unwrap();
    let mut finish = exe.finish_notifier().await?;
    exe.await_init().await.unwrap();

    log::info!("Sending [deploy, start] batch for execution.");

    let batch_id = exe
        .deploy(Some(ProgressArgs {
            update_interval: Some(DurationString::from_str("300ms").unwrap()),
            update_step: None,
        }))
        .await
        .unwrap();

    let msg = activity::StreamExecBatchResults {
        activity_id: config.service_id.unwrap(),
        batch_id: batch_id.clone(),
    };

    // Note: Since we have  already sent commands, we may loose a few events on the beginning.
    // Our API has a problem here. We can't call `StreamExecBatchResults` before Exeunit knows
    // `batch_id`. Even if we would generate id ourselves (possible in test, but not possible for Requestor),
    // we still can't call this function too early.
    let mut stream = bus::service(activity::exeunit::bus_id(&msg.activity_id)).call_streaming(msg);

    let mut last_progress = 0u64;
    while let Some(Ok(Ok(item))) = stream.next().await {
        if item.index == 0 {
            match item.kind {
                RuntimeEventKind::Finished { return_code, .. } => {
                    assert_eq!(return_code, 0);
                    break;
                }
                RuntimeEventKind::Progress(progress) => {
                    log::info!("Progress report: {:?}", progress);

                    assert_eq!(progress.step, (0, 1));
                    assert_eq!(progress.unit, Some("Bytes".to_string()));
                    assert_eq!(progress.progress.1.unwrap(), file_size);
                    assert!(progress.progress.0 >= last_progress);

                    last_progress = progress.progress.0;
                }
                _ => (),
            }
        }
    }

    exe.wait_for_batch(&batch_id).await.unwrap();

    log::info!("Waiting for shutdown..");

    exe.addr.send(Shutdown(ShutdownReason::Finished)).await.ok();
    finish.recv().await.unwrap();
    Ok(())
}
