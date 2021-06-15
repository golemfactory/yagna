use actix::System;
use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use futures::channel::oneshot;
use futures::{StreamExt, TryStreamExt};
use std::env;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use ya_transfer::{transfer_with, HttpTransferProvider, TransferUrl};

const MAX_FAILURES: usize = 2;

lazy_static::lazy_static! {
    static ref FAIL_GET: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    static ref FAIL_PUT: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
}

fn err() -> Result<HttpResponse, actix_web::Error> {
    let kind = std::io::ErrorKind::ConnectionReset;
    let err = std::io::Error::from(kind);
    Err(actix_web::Error::from(err))
}

async fn get() -> Result<HttpResponse, actix_web::Error> {
    let count = { (*FAIL_GET).lock().unwrap().clone() };
    if count < MAX_FAILURES {
        *((*FAIL_GET).lock().unwrap()) = count + 1;
        return err();
    }

    Ok(HttpResponse::Ok().body("ok"))
}

async fn put(payload: web::Payload) -> Result<HttpResponse, actix_web::Error> {
    let count = { (*FAIL_PUT).lock().unwrap().clone() };
    if count < MAX_FAILURES {
        *((*FAIL_PUT).lock().unwrap()) = count + 1;
        return err();
    }

    let _ = payload.into_stream().collect::<Vec<_>>();
    Ok(HttpResponse::Ok().finish())
}

async fn spawn_server(addr: &str) -> anyhow::Result<()> {
    let (tx, rx) = oneshot::channel();
    let addr = addr.to_owned();

    std::thread::spawn(move || {
        let sys = System::new("http");
        HttpServer::new(move || {
            App::new().wrap(middleware::Logger::default()).service(
                web::resource("/{name}")
                    .route(web::get().to(get))
                    .route(web::put().to(put)),
            )
        })
        .bind(addr)
        .expect("unable to start http server")
        .run();

        tx.send(()).expect("channel failed");
        sys.run().expect("sys.run");
    });

    Ok(rx.await?)
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    env::set_var("RUST_LOG", env::var("RUST_LOG").unwrap_or("debug".into()));
    env_logger::init();

    log::debug!("Starting HTTP");

    let srv_addr = "127.0.0.1:8000";
    spawn_server(srv_addr).await?;

    log::debug!("Transferring");

    let src_url = TransferUrl::parse(&format!("http://{}/file_get", srv_addr), "http")?;
    let dst_url = TransferUrl::parse(&format!("http://{}/file_put", srv_addr), "http")?;
    let src = Rc::new(HttpTransferProvider::default());
    let dst = src.clone();

    transfer_with(src, &src_url, dst, &dst_url, &Default::default()).await?;

    log::debug!("Done");

    Ok(())
}
