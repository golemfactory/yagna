use actix_web::{middleware, App, HttpServer};
use ya_persistence::executor::DbExecutor;
use ya_persistence::migrations;
use ya_service_api::constants::{YAGNA_BUS_ADDR, YAGNA_HTTP_ADDR};

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let db = DbExecutor::new(":memory:")?;
    migrations::run_with_output(&db.conn()?, &mut std::io::stdout())?;

    ya_sb_router::bind_router(*YAGNA_BUS_ADDR).await?;
    ya_activity::provider::service::bind_gsb(&db);

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .service(ya_activity::provider::web_scope(&db))
    })
    .bind(*YAGNA_HTTP_ADDR)?
    .run()
    .await?;

    Ok(())
}
