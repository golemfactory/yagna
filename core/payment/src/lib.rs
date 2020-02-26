#![allow(dead_code)] // Crate under development
#![allow(unused_variables)] // Crate under development
use crate::processor::PaymentProcessor;
use ya_payment_driver::DummyDriver;
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::*;

#[macro_use]
extern crate diesel;

pub mod api;
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

lazy_static::lazy_static! {
    // FIXME: Provide real address
    static ref PAYMENT_DRIVER: DummyDriver = DummyDriver::new();
}

pub struct PaymentService;

impl Service for PaymentService {
    type Cli = ();
}

impl PaymentService {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(context: &Context) -> anyhow::Result<()> {
        let db: DbExecutor = context.component();
        db.apply_migration(migrations::run_with_output)?;
        let processor = PaymentProcessor::new(Box::new(PAYMENT_DRIVER.clone()), db.clone());
        self::service::bind_service(&db, processor);
        Ok(())
    }

    pub fn rest(db: &DbExecutor) -> actix_web::Scope {
        let processor = PaymentProcessor::new(Box::new(PAYMENT_DRIVER.clone()), db.clone());
        api::web_scope(db, processor)
    }
}
