use actix_web::{middleware, App, HttpServer};

use ya_activity::{db::migrations, service, TrackerRef};
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::Provider;
use ya_service_api_web::rest_api_addr;

#[derive(Clone)]
struct ServiceContext {
    db: DbExecutor,
    tx: TrackerRef
}

impl<Service> Provider<Service, DbExecutor> for ServiceContext {
    fn component(&self) -> DbExecutor {
        self.db.clone()
    }
}

impl<Service> Provider<Service, TrackerRef> for ServiceContext {
    fn component(&self) -> TrackerRef {
        self.tx.clone()
    }
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let db = DbExecutor::new(":memory:")?;
    db.apply_migration(migrations::run_with_output)?;
    ya_sb_router::bind_gsb_router(None).await?;

    let context = ServiceContext { db: db.clone(), tx: TrackerRef::create() };
    ya_activity::service::Activity::gsb(&context).await?;

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .service(service::Activity::rest(&context))
    })
    .bind(rest_api_addr())?
    .run()
    .await?;

    Ok(())
}
