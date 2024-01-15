use actix_web::dev::ServerHandle;
use actix_web::web::Data;
use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use futures::StreamExt;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

use crate::async_drop::{AsyncDroppable, DroppableTestContext};

pub async fn upload(
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

#[async_trait::async_trait]
impl AsyncDroppable for ServerHandle {
    async fn async_drop(&self) {
        self.stop(true).await;
    }
}

pub async fn start_http(ctx: &mut DroppableTestContext, path: PathBuf) -> anyhow::Result<()> {
    let inner = path.clone();
    let srv = HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .app_data(Data::new(inner.clone()))
            .service(actix_files::Files::new("/", inner.clone()))
    })
    .bind("127.0.0.1:8001")?
    .run();

    ctx.register(srv.handle());
    tokio::task::spawn_local(async move { anyhow::Ok(srv.await?) });

    let inner = path.clone();
    let srv = HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .app_data(Data::new(inner.clone()))
            .service(web::resource("/{name}").route(web::put().to(upload)))
    })
    .bind("127.0.0.1:8002")?
    .run();

    ctx.register(srv.handle());
    tokio::task::spawn_local(async move { anyhow::Ok(srv.await?) });

    Ok(())
}
