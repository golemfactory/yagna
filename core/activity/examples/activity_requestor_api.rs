use actix_web::{middleware, App, HttpServer};

use ya_activity::{db::migrations, service};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::rest_api_addr;

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let db = DbExecutor::new(":memory:")?;
    migrations::run_with_output(&db.conn()?, &mut std::io::stdout())?;

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .service(service::Activity::rest(&db))
    })
    .bind(rest_api_addr())?
    .run()
    .await?;

    Ok(())
}
