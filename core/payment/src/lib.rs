#![allow(dead_code)] // Crate under development
#![allow(unused_variables)] // Crate under development
#![allow(non_local_definitions)] // Due to Diesel macros.

pub use crate::config::Config;
use crate::processor::PaymentProcessor;

use futures::FutureExt;
use std::{sync::Arc, time::Duration};

use ya_core_model::payment::local as pay_local;
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::*;
use ya_service_bus::typed as bus;

#[macro_use]
extern crate diesel;

pub mod accounts;
pub mod api;
mod batch;
mod cli;
pub mod config;
mod cycle;
pub mod dao;
pub mod error;
pub mod models;
pub mod payment_sync;
mod post_migrations;
pub mod processor;
pub mod schema;
pub mod service;
pub mod timeout_lock;
pub mod utils;
mod wallet;

pub use batch::send_batch_payments;

pub mod migrations {
    #[derive(diesel_migrations::EmbedMigrations)]
    struct _Dummy;
}

pub use ya_core_model::payment::local::DEFAULT_PAYMENT_DRIVER;

lazy_static::lazy_static! {
    static ref PAYMENT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(
            std::env::var("PAYMENT_SHUTDOWN_TIMEOUT_SECS")
                .ok()
                .and_then(|x| x.parse().ok())
                .unwrap_or(10),
        );
}

pub struct PaymentService;

impl Service for PaymentService {
    type Cli = cli::PaymentCli;
}

impl PaymentService {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(context: &Context) -> anyhow::Result<()> {
        let db = context.component();
        db.apply_migration(migrations::run_with_output)
            .map_err(|e| anyhow::anyhow!("Failed to apply payment service migrations: {}", e))?;

        let config = Arc::new(Config::from_env()?);

        let processor = Arc::new(PaymentProcessor::new(db.clone()));
        self::service::bind_service(&db, processor.clone(), config).await?;

        processor.process_post_migration_jobs().await?;

        tokio::task::spawn(async move {
            processor.release_allocations(false).await;
        });

        Ok(())
    }

    pub fn rest<Context: Provider<Self, DbExecutor>>(ctx: &Context) -> actix_web::Scope {
        api::web_scope(&ctx.component())
    }

    pub async fn shut_down() {
        log::info!(
            "Stopping payment service... Hit Ctrl+C again to interrupt and shut down immediately."
        );

        let timeout = tokio::time::timeout(
            *PAYMENT_SHUTDOWN_TIMEOUT,
            bus::service(pay_local::BUS_ID)
                .call(pay_local::ShutDown::new(*PAYMENT_SHUTDOWN_TIMEOUT)),
        );

        tokio::select! {
            _ = timeout => {},
            _ = tokio::signal::ctrl_c().boxed() => {},
        }
        log::info!("Payment service stopped.");
    }
}
