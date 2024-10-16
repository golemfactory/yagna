use actix::Actor;
use std::env;
use std::time::Duration;
use test_context::test_context;
use tokio::time::sleep;
use ya_client_model::activity::TransferArgs;
use ya_exe_unit::message::{Shutdown, ShutdownReason};
use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::file::generate_file;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::temp_dir;
use ya_transfer::transfer::{
    AbortTransfers, TransferResource, TransferService, TransferServiceContext,
};

const CHUNK_SIZE: usize = 4096;
const CHUNK_COUNT: usize = 1024 * 25;

async fn interrupted_transfer(
    src: &str,
    dest: &str,
    exe_ctx: TransferServiceContext,
) -> anyhow::Result<()> {
    log::debug!("Starting TransferService");

    let addr = TransferService::new(exe_ctx).start();
    let addr_thread = addr.clone();

    tokio::task::spawn_local(async move {
        sleep(Duration::from_millis(3)).await;

        log::debug!("Aborting transfers");
        let _ = addr_thread.send(AbortTransfers {}).await;
    });

    let response = addr
        .send(TransferResource {
            from: src.to_owned(),
            to: dest.to_owned(),
            args: TransferArgs::default(),
            progress_config: None,
        })
        .await?;

    log::debug!("Response: {:?}", response);
    assert!(response.is_err());

    let _ = addr.send(Shutdown(ShutdownReason::Finished)).await;

    Ok(())
}

#[cfg_attr(not(feature = "system-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_transfer_abort(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(false);

    let temp_dir = temp_dir!("transfer-abort")?;
    let temp_dir = temp_dir.path();

    log::debug!("Creating directories in: {}", temp_dir.display());
    let work_dir = temp_dir.to_owned().join("work_dir");
    let cache_dir = temp_dir.to_owned().join("cache_dir");

    let src_file = temp_dir.join("rnd");
    let dest_file = temp_dir.join("rnd2");

    log::debug!("Starting HTTP");

    let path = temp_dir.to_path_buf();
    ya_framework_basic::server_external::start_http(ctx, path)
        .await
        .expect("unable to start http servers");

    log::debug!("Creating file");

    generate_file(&src_file, CHUNK_SIZE, CHUNK_COUNT);
    let src_size = std::fs::metadata(&src_file)?.len();

    let exe_ctx = TransferServiceContext {
        work_dir: work_dir.clone(),
        cache_dir,
        ..TransferServiceContext::default()
    };

    let _result = interrupted_transfer(
        "http://127.0.0.1:8001/rnd",
        "http://127.0.0.1:8002/rnd2",
        exe_ctx,
    )
    .await;

    let dest_size = match dest_file.exists() {
        true => std::fs::metadata(dest_file)?.len(),
        false => 0u64,
    };
    assert_ne!(src_size, dest_size);

    Ok(())
}
