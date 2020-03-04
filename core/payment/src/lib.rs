#![allow(dead_code)] // Crate under development
#![allow(unused_variables)] // Crate under development
use crate::processor::PaymentProcessor;
use futures::lock::Mutex;
use std::sync::Arc;
use ya_payment_driver::{DummyDriver, PaymentDriver};
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

fn payment_driver_factory() -> impl PaymentDriver {
    DummyDriver::new()
}

lazy_static::lazy_static! {
    // FIXME: Provide real address
    static ref PAYMENT_DRIVER : Arc<Mutex<Box<dyn PaymentDriver + Send + Sync>>>= Arc::new(Mutex::new(Box::new(payment_driver_factory())));
}

pub struct PaymentService;

impl Service for PaymentService {
    type Cli = cli::PaymentCli;
}

impl PaymentService {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(context: &Context) -> anyhow::Result<()> {
        let db: DbExecutor = context.component();
        db.apply_migration(migrations::run_with_output)?;
        let processor = PaymentProcessor::new(PAYMENT_DRIVER.clone(), db.clone());
        self::service::bind_service(&db, processor);
        Ok(())
    }

    pub fn rest(db: &DbExecutor) -> actix_web::Scope {
        let processor = PaymentProcessor::new(PAYMENT_DRIVER.clone(), db.clone());
        api::web_scope(db, processor)
    }
}
