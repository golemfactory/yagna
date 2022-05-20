use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use futures::prelude::*;
use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use structopt::StructOpt;
use tokio::io::AsyncWriteExt;

#[derive(StructOpt, Debug)]
pub struct Cli {
    /// Web server root directory
    #[structopt(short, long)]
    pub root_dir: PathBuf,

    /// Server address
    #[structopt(short, long, default_value = "127.0.0.1:8000")]
    address: SocketAddr,
}

async fn upload(
    path: web::Data<PathBuf>,
    mut payload: web::Payload,
    name: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let mut dst_path = path.as_ref().clone();
    dst_path.push(name.as_ref());

    let mut dst = tokio::fs::File::create(dst_path).await?;
    while let Some(chunk) = payload.next().await {
        let data = chunk.unwrap();
        dst.write_all(&data).await?;
    }

    Ok(HttpResponse::Ok().finish())
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    env::set_var("RUST_LOG", env::var("RUST_LOG").unwrap_or("debug".into()));
    env_logger::init();

    let args: Cli = Cli::from_args();
    log::info!("Web server root directory: {:?}", args.root_dir);

    let root_dir = args.root_dir.clone();
    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .app_data(root_dir.clone())
            .service(web::resource("/upload/{name}").route(web::put().to(upload)))
            .service(actix_files::Files::new("/", root_dir.clone()))
    })
    .bind(args.address)?
    .run()
    .await?;

    Ok(())
}
