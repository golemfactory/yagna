use actix::{Actor, Addr, System};
use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use futures::StreamExt;
use rand::Rng;
use serde_json::Value;
use sha3::digest::generic_array::GenericArray;
use sha3::Digest;
use std::collections::HashMap;
use std::env;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tempdir::TempDir;
use tokio::io::AsyncWriteExt;
use ya_agreement_utils::AgreementView;
use ya_client_model::activity::TransferArgs;
use ya_exe_unit::agreement::Agreement;
use ya_exe_unit::error::Error;
use ya_exe_unit::service::transfer::{AddVolumes, DeployImage, TransferResource, TransferService};
use ya_exe_unit::ExeUnitContext;
use ya_runtime_api::deploy::ContainerVolume;

type HashOutput = GenericArray<u8, <sha3::Sha3_512 as Digest>::OutputSize>;

fn create_file(path: &Path, name: &str, chunk_size: usize, chunk_count: usize) -> HashOutput {
    let path = path.join(name);
    let mut hasher = sha3::Sha3_512::default();
    let mut file_src = OpenOptions::new()
        .write(true)
        .create(true)
        .open(path)
        .expect("rnd file");

    let mut rng = rand::thread_rng();

    for _ in 0..chunk_count {
        let input: Vec<u8> = (0..chunk_size)
            .map(|_| rng.gen_range(0, 256) as u8)
            .collect();

        hasher.input(&input);
        let _ = file_src.write(&input).unwrap();
    }
    file_src.flush().unwrap();
    hasher.result()
}

fn hash_file(path: &Path) -> HashOutput {
    let mut file_src = OpenOptions::new().read(true).open(path).expect("rnd file");

    let mut hasher = sha3::Sha3_512::default();
    let mut chunk = vec![0; 4096];

    while let Ok(count) = file_src.read(&mut chunk[..]) {
        hasher.input(&chunk[..count]);
        if count != 4096 {
            break;
        }
    }
    hasher.result()
}

async fn upload(
    path: web::Data<PathBuf>,
    mut payload: web::Payload,
    name: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let mut dst_path = path.as_ref().clone();
    dst_path.push(name.as_ref());

    let mut dst = tokio::fs::File::create(dst_path).await.unwrap();

    while let Some(chunk) = payload.next().await {
        let data = chunk.unwrap();
        dst.write_all(&data).await?;
    }

    Ok(HttpResponse::Ok().finish())
}

async fn start_http(path: PathBuf) -> anyhow::Result<()> {
    let inner = path.clone();
    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .app_data(inner.clone())
            .service(actix_files::Files::new("/", inner.clone()))
    })
    .bind("127.0.0.1:8001")?
    .run()
    .await?;

    let inner = path.clone();
    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .app_data(inner.clone())
            .service(web::resource("/{name}").route(web::put().to(upload)))
    })
    .bind("127.0.0.1:8002")?
    .run()
    .await?;

    Ok(())
}

#[cfg(feature = "sgx")]
fn init_crypto() -> anyhow::Result<ya_exe_unit::crypto::Crypto> {
    use ya_exe_unit::crypto::Crypto;

    // dummy impl
    let ec = secp256k1::Secp256k1::new();
    let (sec_key, req_key) = ec.generate_keypair(&mut rand::thread_rng());
    Ok(Crypto::try_with_keys_raw(sec_key, req_key)?)
}

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

fn verify_hash<S: AsRef<str>, P: AsRef<Path>>(hash: &HashOutput, path: P, file_name: S) {
    let path = path.as_ref().join(file_name.as_ref());
    log::info!("Verifying hash of {:?}", path);
    assert_eq!(hash, &hash_file(&path));
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    env::set_var(
        "RUST_LOG",
        env::var("RUST_LOG").unwrap_or_else(|_| "debug".into()),
    );
    env_logger::init();

    log::debug!("Creating directories");
    let dir = TempDir::new("transfer")?;
    let temp_dir = dir.path();
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

    let chunk_size = 4096 as usize;
    let chunk_count = 256 as usize;

    log::debug!(
        "Creating a random file of size {} * {}",
        chunk_size,
        chunk_count
    );
    let hash = create_file(temp_dir, "rnd", chunk_size, chunk_count);

    log::debug!("Starting HTTP servers");

    let path = temp_dir.to_path_buf();
    tokio::task::spawn_local(async move {
        let sys = System::new();
        start_http(path)
            .await
            .expect("unable to start http servers");
        sys.run().expect("sys.run");
    });

    let agreement = Agreement {
        inner: AgreementView {
            agreement_id: String::new(),
            json: Value::Null,
        },
        task_package: Some(format!(
            "hash://sha3:{}:http://127.0.0.1:8001/rnd",
            hex::encode(hash)
        )),
        usage_vector: Vec::new(),
        usage_limits: HashMap::new(),
        infrastructure: HashMap::new(),
    };

    log::debug!("Starting TransferService");
    let exe_ctx = ExeUnitContext {
        supervise: Default::default(),
        activity_id: None,
        acl: Default::default(),
        report_url: None,
        credentials: None,
        agreement,
        work_dir: work_dir.clone(),
        cache_dir,
        runtime_args: Default::default(),
        #[cfg(feature = "sgx")]
        crypto: init_crypto()?,
    };
    let transfer_service = TransferService::new(&exe_ctx);
    let addr = transfer_service.start();

    log::debug!("Adding volumes");
    addr.send(AddVolumes::new(volumes)).await??;

    println!();
    log::warn!("[>>] Deployment with hash verification");
    addr.send(DeployImage {}).await??;
    log::warn!("Deployment complete");

    println!();
    log::warn!("[>>] Deployment from cache");
    addr.send(DeployImage {}).await??;
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
    verify_hash(&hash, &work_dir.join("vol-3"), "rnd-1");
    verify_hash(&hash, &work_dir.join("vol-3"), "rnd-4");
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
