use duration_string::DurationString;
use futures::StreamExt;
use std::str::FromStr;
use test_context::test_context;

use ya_client_model::activity::exe_script_command::ProgressArgs;
use ya_client_model::activity::{ExeScriptCommand, RuntimeEventKind, TransferArgs};
use ya_core_model::activity;
use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::file::generate_image;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::server_external::start_http;
use ya_framework_basic::test_dirs::{cargo_binary, template};
use ya_framework_basic::{resource, temp_dir};
use ya_mock_runtime::testing::{create_exe_unit, exe_unit_config, ExeUnitExt};

use ya_service_bus::typed as bus;

/// Test if progress reporting mechanisms work on gsb level
/// with full ExeUnit setup.
#[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_progress_reporting(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("progress-reporting")?;
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
    exe.await_init().await.unwrap();

    log::info!("Sending [deploy] batch for execution.");

    let batch_id = exe
        .exec(
            None,
            vec![ExeScriptCommand::Deploy {
                net: vec![],
                progress: Some(ProgressArgs {
                    update_interval: Some(DurationString::from_str("300ms").unwrap()),
                    update_step: None,
                }),
                env: Default::default(),
                hosts: Default::default(),
                hostname: None,
                volumes: vec!["/input".to_owned()],
            }],
        )
        .await
        .unwrap();

    validate_progress(
        config.service_id.clone().unwrap(),
        batch_id.clone(),
        file_size,
    )
    .await;

    exe.wait_for_batch(&batch_id).await.unwrap();
    exe.wait_for_batch(&exe.start(vec![]).await.unwrap())
        .await
        .unwrap();

    let batch_id = exe
        .exec(
            None,
            vec![ExeScriptCommand::Transfer {
                args: TransferArgs::default(),
                progress: Some(ProgressArgs {
                    update_interval: Some(DurationString::from_str("100ms").unwrap()),
                    update_step: None,
                }),
                // Important: Use hashed transfer, because it is significantly slower in debug mode.
                // Otherwise we won't get any progress message, because it is too fast.
                from: format!(
                    "hash://sha3:{}:http://127.0.0.1:8001/image-big",
                    hex::encode(&hash)
                ),
                to: "container:/input/image-copy".to_string(),
            }],
        )
        .await
        .unwrap();

    validate_progress(config.service_id.unwrap(), batch_id.clone(), file_size).await;
    exe.wait_for_batch(&batch_id).await.unwrap();
    Ok(())
}

async fn validate_progress(activity_id: String, batch_id: String, file_size: u64) {
    let msg = activity::StreamExecBatchResults {
        activity_id: activity_id.clone(),
        batch_id: batch_id.clone(),
    };

    // Note: Since we have  already sent commands, we may loose a few events on the beginning.
    // Our API has a problem here. We can't call `StreamExecBatchResults` before Exeunit knows
    // `batch_id`. Even if we would generate id ourselves (possible in test, but not possible for Requestor),
    // we still can't call this function too early.
    let mut stream = bus::service(activity::exeunit::bus_id(&msg.activity_id)).call_streaming(msg);

    let mut last_progress = 0u64;
    let mut num_progresses = 0u64;
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
                    num_progresses += 1;
                }
                _ => (),
            }
        }
    }
    assert!(num_progresses > 1);
}
