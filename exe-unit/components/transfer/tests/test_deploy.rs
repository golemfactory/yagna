use actix_web::{middleware, web, App, HttpServer};
use std::path::PathBuf;

// async fn start_http(path: PathBuf) -> anyhow::Result<()> {
//     let inner = path.clone();
//     HttpServer::new(move || {
//         App::new()
//             .wrap(middleware::Logger::default())
//             .app_data(inner.clone())
//             .service(actix_files::Files::new("/", inner.clone()))
//     })
//     .bind("127.0.0.1:8001")?
//     .run()
//     .await?;
//
//     let inner = path.clone();
//     HttpServer::new(move || {
//         App::new()
//             .wrap(middleware::Logger::default())
//             .app_data(inner.clone())
//             .service(web::resource("/{name}").route(web::put().to(upload)))
//     })
//     .bind("127.0.0.1:8002")?
//     .run()
//     .await?;
//
//     Ok(())
// }

#[cfg_attr(not(feature = "framework-test"), ignore)]
#[actix_rt::test]
async fn test_deploy_image() -> anyhow::Result<()> {
    Ok(())
}
