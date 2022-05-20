//scx - it looks like this api needs tracker - commeting out
/*
use actix_web::{middleware, App, HttpServer};

use ya_activity::{db::migrations, service};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::rest_api_addr;
*/

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    //scx - it looks like this api needs tracker - commeting out
    /*
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let db = DbExecutor::new(":memory:")?;
    db.apply_migration(migrations::run_with_output)?;

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .service(service::Activity::rest(&db))
    })
    .bind(rest_api_addr())?
    .run()
    .await?;*/

    Ok(())
}
