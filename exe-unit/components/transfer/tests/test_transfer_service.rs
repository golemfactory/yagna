use actix::{Actor, Addr};
use std::env;
use test_context::test_context;

use ya_client_model::activity::TransferArgs;
use ya_exe_unit::error::Error;
use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::file::generate_file_with_hash;
use ya_framework_basic::hash::verify_hash;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::server_external::start_http;
use ya_framework_basic::temp_dir;
use ya_runtime_api::deploy::ContainerVolume;
use ya_transfer::transfer::{
    AddVolumes, DeployImage, TransferResource, TransferService, TransferServiceContext,
};

async fn transfer(addr: &Addr<TransferService>, from: &str, to: &str) -> Result<(), Error> {
    transfer_with_args(addr, from, to, TransferArgs::default()).await
}

async fn transfer_with_args(
    addr: &Addr<TransferService>,
    from: &str,
    to: &str,
    args: TransferArgs,
) -> Result<(), Error> {
    log::info!("Triggering transfer from {} to {}", from, to);

    addr.send(TransferResource {
        from: from.to_owned(),
        to: to.to_owned(),
        args,
    })
    .await??;

    Ok(())
}

#[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_transfer_scenarios(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("transfer")?;
    let temp_dir = dir.path();

    log::debug!("Creating directories in: {}", temp_dir.display());
    let work_dir = temp_dir.join("work_dir");
    let cache_dir = temp_dir.join("cache_dir");
    let sub_dir = temp_dir.join("sub_dir");

    for dir in vec![work_dir.clone(), cache_dir.clone(), sub_dir.clone()] {
        std::fs::create_dir_all(dir)?;
    }
    let volumes = vec![
        ContainerVolume {
            name: "vol-1".into(),
            path: "/input".into(),
        },
        ContainerVolume {
            name: "vol-2".into(),
            path: "/output".into(),
        },
        ContainerVolume {
            name: "vol-3".into(),
            path: "/extract".into(),
        }, // Uncomment to enable logs
    ];

    let chunk_size = 4096_usize;
    let chunk_count = 256_usize;

    log::debug!(
        "Creating a random file of size {} * {}",
        chunk_size,
        chunk_count
    );
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

    log::debug!("Adding volumes");
    addr.send(AddVolumes::new(volumes)).await??;

    println!();
    log::warn!("[>>] Deployment with hash verification");
    addr.send(DeployImage {
        task_package: task_package.clone(),
    })
    .await??;
    log::warn!("Deployment complete");

    println!();
    log::warn!("[>>] Deployment from cache");
    addr.send(DeployImage {
        task_package: task_package.clone(),
    })
    .await??;
    log::warn!("Deployment from cache complete");

    println!();
    log::warn!("[>>] Transfer HTTP -> container");
    transfer(&addr, "http://127.0.0.1:8001/rnd", "container:/input/rnd-1").await?;
    verify_hash(&hash, work_dir.join("vol-1"), "rnd-1");
    log::warn!("Checksum verified");

    println!();
    log::warn!("[>>] Transfer container -> HTTP");
    transfer(
        &addr,
        "container:/input/rnd-1",
        "http://127.0.0.1:8002/rnd-2",
    )
    .await?;
    verify_hash(&hash, temp_dir, "rnd-2");
    log::warn!("Checksum verified");

    println!();
    log::warn!("[>>] Transfer HTTP -> HTTP");
    transfer(
        &addr,
        "http://127.0.0.1:8001/rnd",
        "http://127.0.0.1:8002/rnd-3",
    )
    .await?;
    verify_hash(&hash, temp_dir, "rnd-3");
    log::warn!("Checksum verified");

    println!();
    log::warn!("[>>] Transfer container -> container");
    transfer(&addr, "container:/input/rnd-1", "container:/input/rnd-4").await?;
    verify_hash(&hash, work_dir.join("vol-1"), "rnd-4");
    log::warn!("Checksum verified");

    println!();
    log::warn!("[>>] Transfer container -> container (different volume)");
    transfer(&addr, "container:/input/rnd-1", "container:/output/rnd-5").await?;
    verify_hash(&hash, work_dir.join("vol-2"), "rnd-5");
    log::warn!("Checksum verified");

    Ok(())
}

#[ignore]
#[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_transfer_archived(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("transfer-archive")?;
    let temp_dir = dir.path();

    log::debug!("Creating directories in: {}", temp_dir.display());
    let work_dir = temp_dir.join("work_dir");
    let cache_dir = temp_dir.join("cache_dir");
    let sub_dir = temp_dir.join("sub_dir");

    for dir in vec![work_dir.clone(), cache_dir.clone(), sub_dir.clone()] {
        std::fs::create_dir_all(dir)?;
    }
    let volumes = vec![
        ContainerVolume {
            name: "vol-1".into(),
            path: "/input".into(),
        },
        ContainerVolume {
            name: "vol-2".into(),
            path: "/output".into(),
        },
        ContainerVolume {
            name: "vol-3".into(),
            path: "/extract".into(),
        },
    ];

    let chunk_size = 4096_usize;
    let chunk_count = 256_usize;

    log::debug!(
        "Creating a random file of size {} * {}",
        chunk_size,
        chunk_count
    );
    let hash = generate_file_with_hash(temp_dir, "rnd", chunk_size, chunk_count);

    log::debug!("Starting HTTP servers");

    let path = temp_dir.to_path_buf();
    start_http(ctx, path)
        .await
        .expect("unable to start http servers");

    log::debug!("Starting TransferService");
    let exe_ctx = TransferServiceContext {
        work_dir: work_dir.clone(),
        cache_dir,
        ..TransferServiceContext::default()
    };
    let addr = TransferService::new(exe_ctx).start();

    log::debug!("Adding volumes");
    addr.send(AddVolumes::new(volumes)).await??;

    println!();
    log::warn!("[>>] Transfer container (archive TAR.GZ) -> HTTP");
    let args = TransferArgs {
        format: Some(String::from("tar.gz")),
        ..Default::default()
    };
    // args.fileset = Some(FileSet::Pattern(SetEntry::Single("**/rnd-*".into())));
    transfer_with_args(
        &addr,
        "container:/input",
        "http://127.0.0.1:8002/input.tar.gz",
        args,
    )
    .await?;
    let output_path = temp_dir.join("input.tar.gz");
    assert!(output_path.is_file());
    assert!(std::fs::metadata(&output_path)?.len() > 0);
    log::warn!("Compression complete");

    println!();
    log::warn!("[>>] Transfer container (archive ZIP) -> HTTP");
    let args = TransferArgs {
        format: Some(String::from("zip")),
        ..Default::default()
    };
    // args.fileset = Some(FileSet::Pattern(SetEntry::Single("**/rnd-*".into())));
    transfer_with_args(
        &addr,
        "container:/input",
        "http://127.0.0.1:8002/input.zip",
        args,
    )
    .await?;
    let output_path = temp_dir.join("input.zip");
    assert!(output_path.is_file());
    assert!(std::fs::metadata(&output_path)?.len() > 0);
    log::warn!("Compression complete");

    println!();
    log::warn!("[>>] Transfer HTTP -> container (extract TAR.GZ)");
    let args = TransferArgs {
        format: Some(String::from("tar.gz")),
        ..Default::default()
    };
    transfer_with_args(
        &addr,
        "http://127.0.0.1:8001/input.tar.gz",
        "container:/extract",
        args,
    )
    .await?;
    log::warn!("Extraction complete");
    verify_hash(&hash, work_dir.join("vol-3"), "rnd-1");
    verify_hash(&hash, work_dir.join("vol-3"), "rnd-4");
    log::warn!("Checksum verified");

    log::warn!("Removing extracted files");
    std::fs::remove_file(work_dir.join("vol-3").join("rnd-1"))?;
    std::fs::remove_file(work_dir.join("vol-3").join("rnd-4"))?;

    println!();
    log::warn!("[>>] Transfer HTTP -> container (extract ZIP)");
    let args = TransferArgs {
        format: Some(String::from("zip")),
        ..Default::default()
    };
    transfer_with_args(
        &addr,
        "http://127.0.0.1:8001/input.zip",
        "container:/extract",
        args,
    )
    .await?;
    log::warn!("Extraction complete");
    verify_hash(&hash, work_dir.join("vol-3"), "rnd-1");
    verify_hash(&hash, work_dir.join("vol-3"), "rnd-4");
    log::warn!("Checksum verified");

    log::warn!("Removing extracted files");
    std::fs::remove_file(work_dir.join("vol-3").join("rnd-1"))?;
    std::fs::remove_file(work_dir.join("vol-3").join("rnd-4"))?;

    println!();
    log::warn!("[>>] Transfer container (archive TAR.GZ) -> container (extract TAR.GZ)");
    let args = TransferArgs {
        format: Some(String::from("tar.gz")),
        ..Default::default()
    };
    // args.fileset = Some(FileSet::Pattern(SetEntry::Single("**/rnd-*".into())));
    transfer_with_args(&addr, "container:/input", "container:/extract", args).await?;
    log::warn!("Transfer complete");
    verify_hash(&hash, work_dir.join("vol-3"), "rnd-1");
    verify_hash(&hash, work_dir.join("vol-3"), "rnd-4");
    log::warn!("Checksum verified");

    transfer(
        &addr,
        "https://www.rust-lang.org",
        "http://127.0.0.1:8002/index.html",
    )
    .await
    .expect("HTTPS transfer failed");

    Ok(())
}
