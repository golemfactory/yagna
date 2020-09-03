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
use ya_agreement_utils::AgreementView;
use ya_exe_unit::agreement::Agreement;
use ya_exe_unit::error::Error;
use ya_exe_unit::runtime::RuntimeArgs;
use ya_exe_unit::service::transfer::{DeployImage, TransferResource, TransferService};
use ya_exe_unit::ExeUnitContext;

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
        file_src.write(&input).unwrap();
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

    let mut dst = web::block(|| std::fs::File::create(dst_path))
        .await
        .unwrap();

    while let Some(chunk) = payload.next().await {
        let data = chunk.unwrap();
        dst = web::block(move || dst.write_all(&data).map(|_| dst)).await?;
    }

    Ok(HttpResponse::Ok().finish())
}

fn start_http(path: PathBuf) -> anyhow::Result<()> {
    let inner = path.clone();
    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .data(inner.clone())
            .service(actix_files::Files::new("/", inner.clone()))
    })
    .bind("127.0.0.1:8001")?
    .run();

    let inner = path.clone();
    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .data(inner.clone())
            .service(web::resource("/{name}").route(web::put().to(upload)))
    })
    .bind("127.0.0.1:8002")?
    .run();

    Ok(())
}

async fn transfer(addr: &Addr<TransferService>, from: &str, to: &str) -> Result<(), Error> {
    log::info!("Triggering transfer from {} to {}", from, to);

    addr.send(TransferResource {
        from: from.to_owned(),
        to: to.to_owned(),
    })
    .await??;

    Ok(())
}

fn verify_hash<S: AsRef<str>>(hash: &HashOutput, path: &Path, file_name: S) {
    let path = path.clone().join(file_name.as_ref());
    log::info!("Verifying hash of {:?}", path);
    assert_eq!(hash, &hash_file(&path));
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    env::set_var("RUST_LOG", env::var("RUST_LOG").unwrap_or("debug".into()));
    env_logger::init();

    log::debug!("Creating directories");
    let temp_dir = TempDir::new("transfer").unwrap();
    let work_dir = temp_dir.path().clone().join("work_dir");
    let cache_dir = temp_dir.path().clone().join("cache_dir");
    let sub_dir = temp_dir.path().clone().join("sub_dir");

    for dir in vec![&work_dir, &cache_dir, &sub_dir] {
        std::fs::create_dir_all(dir)?;
    }

    let chunk_size = 4096 as usize;
    let chunk_count = 256 as usize;

    log::debug!(
        "Creating a random file of size {} * {}",
        chunk_size,
        chunk_count
    );
    let hash = create_file(temp_dir.path(), "rnd", chunk_size, chunk_count);

    log::debug!("Starting HTTP servers");

    let path = temp_dir.path().to_path_buf();
    std::thread::spawn(move || {
        let sys = System::new("http");
        start_http(path).expect("unable to start http servers");
        sys.run().expect("sys.run");
    });

    let agreement = Agreement {
        inner: AgreementView {
            agreement_id: String::new(),
            json: Value::Null,
        },
        task_package: format!(
            "hash://sha3:{}:http://127.0.0.1:8001/rnd",
            hex::encode(hash)
        ),
        usage_vector: Vec::new(),
        usage_limits: HashMap::new(),
        infrastructure: HashMap::new(),
    };

    log::debug!("Starting TransferService");
    let runtime_args = RuntimeArgs::new(&work_dir, &agreement, true);
    let exe_ctx = ExeUnitContext {
        activity_id: None,
        report_url: None,
        agreement,
        work_dir: work_dir.clone(),
        cache_dir,
        runtime_args,
    };
    let transfer_service = TransferService::new(&exe_ctx);
    let addr = transfer_service.start();

    log::warn!("Deploy with transfer and hash check");
    addr.send(DeployImage {})
        .await
        .expect("send failed")
        .expect("deployment failed");
    log::warn!("Deployment complete");

    log::warn!("Deploy from cache");
    addr.send(DeployImage {})
        .await
        .expect("send failed")
        .expect("deployment failed");
    log::warn!("Deployment from cache complete");

    transfer(
        &addr,
        "http://127.0.0.1:8001/rnd",
        "http://127.0.0.1:8002/rnd_upload",
    )
    .await
    .expect("transfer failed");
    verify_hash(&hash, temp_dir.path(), "rnd_upload");
    log::warn!("Verification complete");

    transfer(
        &addr,
        "https://www.rust-lang.org",
        "http://127.0.0.1:8002/index.html",
    )
    .await
    .expect("HTTPS transfer failed");

    Ok(())
}
