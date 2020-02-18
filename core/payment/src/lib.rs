#![allow(dead_code)] // Crate under development
#![allow(unused_variables)] // Crate under development
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::*;

#[macro_use]
extern crate diesel;

pub mod api;
pub mod dao;
pub mod error;
pub mod models;
pub mod schema;
pub mod service;
pub mod utils;

pub mod migrations {
    #[derive(diesel_migrations::EmbedMigrations)]
    struct _Dummy;
}

struct PaymentService;

impl Service for PaymentService {
    type Cli = ();
}

impl PaymentService {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(context: &Context) -> anyhow::Result<()> {
        let db: DbExecutor = context.component();
        db.apply_migration(migrations::run_with_output)?;

        self::service::bind_service(&db);
        Ok(())
    }
}
