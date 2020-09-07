use actix::{Actor, System};
use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use futures::StreamExt;
use rand::Rng;
use std::collections::HashMap;
use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use tempdir::TempDir;
use tokio::time::delay_for;
use ya_agreement_utils::AgreementView;
use ya_client_model::activity::TransferArgs;
use ya_exe_unit::agreement::Agreement;
use ya_exe_unit::message::{Shutdown, ShutdownReason};
use ya_exe_unit::runtime::RuntimeArgs;
use ya_exe_unit::service::transfer::{AbortTransfers, TransferResource, TransferService};
use ya_exe_unit::ExeUnitContext;

const CHUNK_SIZE: usize = 4096;
const CHUNK_COUNT: usize = 1024 * 25;

fn create_file(path: &PathBuf) {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .open(path)
        .expect("rnd file");

    let mut rng = rand::thread_rng();
    let input: Vec<u8> = (0..CHUNK_SIZE)
        .map(|_| rng.gen_range(0, 256) as u8)
        .collect();

    for _ in 0..CHUNK_COUNT {
        file.write(&input).unwrap();
    }
    file.flush().unwrap();
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

async fn interrupted_transfer(
    src: &str,
    dest: &str,
    exe_ctx: &ExeUnitContext,
) -> anyhow::Result<()> {
    let transfer_service = TransferService::new(&exe_ctx);
    let addr = transfer_service.start();
    let addr_thread = addr.clone();

    std::thread::spawn(move || {
        System::new("transfer").block_on(async move {
            delay_for(Duration::from_millis(10)).await;
            let _ = addr_thread.send(AbortTransfers {}).await;
        })
    });

    let response = addr
        .send(TransferResource {
            from: src.to_owned(),
            to: dest.to_owned(),
            args: TransferArgs::default(),
        })
        .await?;

    assert!(response.is_err());
    log::debug!("Response: {:?}", response);

    let _ = addr.send(Shutdown(ShutdownReason::Finished)).await;

    Ok(())
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    env::set_var("RUST_LOG", env::var("RUST_LOG").unwrap_or("debug".into()));
    env_logger::init();

    log::debug!("Creating directories");

    let temp_dir = TempDir::new("transfer")?;
    let work_dir = temp_dir.path().clone().join("work_dir");
    let cache_dir = temp_dir.path().clone().join("cache_dir");

    let src_file = temp_dir.path().join("rnd");
    let dest_file = temp_dir.path().join("rnd2");

    log::debug!("Starting HTTP");

    let path = temp_dir.path().to_path_buf();
    std::thread::spawn(move || {
        let sys = System::new("http");
        start_http(path).expect("unable to start http servers");
        sys.run().expect("sys.run");
    });

    log::debug!("Creating file");

    create_file(&src_file);
    let src_size = std::fs::metadata(&src_file)?.len();

    let agreement = Agreement {
        inner: AgreementView {
            agreement_id: String::new(),
            json: serde_json::Value::Null,
        },
        task_package: "".to_string(),
        usage_vector: Vec::new(),
        usage_limits: HashMap::new(),
        infrastructure: HashMap::new(),
    };

    let runtime_args = RuntimeArgs::new(&work_dir, &agreement, true);
    let exe_ctx = ExeUnitContext {
        activity_id: None,
        report_url: None,
        agreement,
        work_dir,
        cache_dir,
        runtime_args,
    };

    let _result = interrupted_transfer(
        "http://127.0.0.1:8001/rnd",
        "http://127.0.0.1:8002/rnd2",
        &exe_ctx,
    )
    .await;

    let dest_size = match dest_file.exists() {
        true => std::fs::metadata(dest_file)?.len(),
        false => 0u64,
    };
    assert_ne!(src_size, dest_size);

    Ok(())
}
