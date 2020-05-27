#![allow(dead_code)] // Crate under development
#![allow(unused_variables)] // Crate under development
use crate::processor::PaymentProcessor;
use ya_payment_driver::PaymentDriver;
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::*;

#[macro_use]
extern crate diesel;

pub mod api;
mod cli;
pub mod dao;
pub mod error;
pub mod models;
pub mod processor;
pub mod schema;
pub mod service;
pub mod utils;

pub mod migrations {
    #[derive(diesel_migrations::EmbedMigrations)]
    struct _Dummy;
}

#[cfg(feature = "dummy-driver")]
async fn payment_driver_factory(db: &DbExecutor) -> anyhow::Result<impl PaymentDriver> {
    use ya_payment_driver::DummyDriver;

    Ok(DummyDriver::new())
}

#[cfg(feature = "gnt-driver")]
async fn payment_driver_factory(db: &DbExecutor) -> anyhow::Result<impl PaymentDriver> {
    use ya_payment_driver::GntDriver;

    Ok(GntDriver::new(db.clone()).await?)
}

pub struct PaymentService;

impl Service for PaymentService {
    type Cli = cli::PaymentCli;
}

impl PaymentService {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(context: &Context) -> anyhow::Result<()> {
        let db: DbExecutor = context.component();
        db.apply_migration(migrations::run_with_output)?;
        let driver = payment_driver_factory(&db).await?;
        let processor = PaymentProcessor::new(driver, db.clone());
        self::service::bind_service(&db, processor);
        Ok(())
    }

    pub fn rest<Context: Provider<Self, DbExecutor>>(ctx: &Context) -> actix_web::Scope {
        api::web_scope(&ctx.component())
    }
}
