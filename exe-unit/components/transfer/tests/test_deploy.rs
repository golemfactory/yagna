use actix::Actor;
use std::env;
use std::time::Duration;
use test_context::test_context;
use tokio::time::sleep;

use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::file::generate_file_with_hash;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::server_external::start_http;
use ya_framework_basic::temp_dir;
use ya_transfer::transfer::{AbortTransfers, DeployImage, TransferService, TransferServiceContext};

/// When re-deploying image, `TransferService` should uses partially downloaded image.
/// Hash computations should be correct in both cases.
#[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_deploy_image_restart(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(true);

    let dir = temp_dir!("deploy-restart")?;
    let temp_dir = dir.path();

    log::debug!("Creating directories in: {}", temp_dir.display());
    let work_dir = temp_dir.join("work_dir");
    let cache_dir = temp_dir.join("cache_dir");
    let sub_dir = temp_dir.join("sub_dir");

    for dir in vec![work_dir.clone(), cache_dir.clone(), sub_dir.clone()] {
        std::fs::create_dir_all(dir)?;
    }

    let chunk_size = 4096_usize;
    let chunk_count = 1024 * 10;

    log::debug!("Creating a random file of size {chunk_size} * {chunk_count}");
    let hash = generate_file_with_hash(temp_dir, "rnd", chunk_size, chunk_count);

    log::debug!("Starting HTTP servers");
    let path = temp_dir.to_path_buf();
    start_http(ctx, path)
        .await
        .expect("unable to start http servers");

    let task_package = Some(format!(
        "hash://sha3:{}:http://127.0.0.1:8001/rnd",
        hex::encode(hash)
    ));

    log::debug!("Starting TransferService");
    let exe_ctx = TransferServiceContext {
        work_dir: work_dir.clone(),
        cache_dir,
        ..TransferServiceContext::default()
    };
    let addr = TransferService::new(exe_ctx).start();
    let addr_ = addr.clone();

    tokio::task::spawn_local(async move {
        sleep(Duration::from_millis(3)).await;

        log::debug!("Aborting transfers");
        let _ = addr_.send(AbortTransfers {}).await;
    });

    log::info!("[>>] Deployment with hash verification");
    let result = addr
        .send(DeployImage {
            task_package: task_package.clone(),
        })
        .await?;
    log::info!("Deployment stopped");

    assert!(result.is_err());

    log::info!("Re-deploying the same image");
    addr.send(DeployImage {
        task_package: task_package.clone(),
    })
    .await??;

    Ok(())
}
