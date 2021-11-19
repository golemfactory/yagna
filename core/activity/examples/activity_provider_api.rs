use actix_web::{middleware, App, HttpServer};

use ya_activity::{db::migrations, service};
use ya_service_api_interfaces::Provider;
use ya_service_api_web::rest_api_addr;

struct ServiceContext {
    db: DbExecutor,
}

impl<Service> Provider<Service, DbExecutor> for ServiceContext {
    fn component(&self) -> DbExecutor {
        self.db.clone()
    }
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let db = DbExecutor::new(":memory:")?;
    db.apply_migration(migrations::run_with_output)?;
    ya_sb_router::bind_gsb_router(None).await?;

    let context = ServiceContext { db: db.clone() };
    ya_activity::service::Activity::gsb(&context).await?;

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
