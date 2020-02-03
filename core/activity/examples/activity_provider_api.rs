use actix_web::{middleware, App, HttpServer};
use ya_persistence::executor::DbExecutor;
use ya_persistence::migrations;
use ya_service_api::constants::{YAGNA_BUS_ADDR, YAGNA_HTTP_ADDR};
use ya_service_api_web::scope::ExtendableScope;

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let db = DbExecutor::new(":memory:")?;
    migrations::run_with_output(&db.conn()?, &mut std::io::stdout())?;

    ya_sb_router::bind_router(*YAGNA_BUS_ADDR).await?;
    ya_activity::provider::service::bind_gsb(&db);

    HttpServer::new(move || {
        let activity = actix_web::web::scope(ya_activity::ACTIVITY_API_PATH)
            .data(db.clone())
            .extend(ya_activity::provider::extend_web_scope);

        App::new()
            .wrap(middleware::Logger::default())
            .service(activity)
    })
    .bind(*YAGNA_HTTP_ADDR)?
    .run()
    .await?;

    Ok(())
}
