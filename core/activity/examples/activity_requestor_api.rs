use actix_web::{middleware, App, HttpServer};
use ya_activity::requestor;
use ya_persistence::executor::DbExecutor;
use ya_persistence::migrations;
use ya_service_api_web::{rest_api_addr, scope::ExtendableScope};

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let db = DbExecutor::new(":memory:")?;
    migrations::run_with_output(&db.conn()?, &mut std::io::stdout())?;

    HttpServer::new(move || {
        let activity = actix_web::web::scope(ya_activity::ACTIVITY_API_PATH)
            .data(db.clone())
            .extend(requestor::control::extend_web_scope)
            .extend(requestor::state::extend_web_scope);

        App::new()
            .wrap(middleware::Logger::default())
            .service(activity)
    })
    .bind(rest_api_addr())?
    .run()
    .await?;

    Ok(())
}
