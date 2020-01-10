use futures::lock::Mutex;
use std::sync::Arc;

use actix_web::{middleware, App, HttpServer};
use ya_activity::requestor;
use ya_persistence::executor::DbExecutor;
use ya_persistence::migrations;

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let db = Arc::new(Mutex::new(DbExecutor::new(":memory:")?));
    migrations::run_with_output(&db.lock().await.conn()?, &mut std::io::stdout())?;

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .service(requestor::control::web_scope(db.clone()))
            .service(requestor::state::web_scope(db.clone()))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await?;

    Ok(())
}
