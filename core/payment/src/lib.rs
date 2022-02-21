#![allow(dead_code)] // Crate under development
#![allow(unused_variables)] // Crate under development
use crate::processor::PaymentProcessor;
use actix_web::web::Data;
use futures::FutureExt;
use std::time::Duration;
use ya_core_model::payment::local as pay_local;
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::*;
use ya_service_bus::typed as bus;

#[macro_use]
extern crate diesel;

pub mod accounts;
pub mod api;
mod cli;
pub mod dao;
pub mod error;
pub mod models;
pub mod processor;
pub mod schema;
pub mod service;
pub mod utils;
mod wallet;

pub mod migrations {
    #[derive(diesel_migrations::EmbedMigrations)]
    struct _Dummy;
}

pub const DEFAULT_PAYMENT_PLATFORM: &str = "erc20-rinkeby-tglm";

use crate::api::allocations::release_allocation_after;
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
        let db = Data::new(context.clone().component());
        release_allocations(db.clone()).await;
        let db = context.component();
        db.apply_migration(migrations::run_with_output)?;
        let processor = PaymentProcessor::new(db.clone());
        self::service::bind_service(&db, processor);
        Ok(())
    }

    pub fn rest<Context: Provider<Self, DbExecutor>>(ctx: &Context) -> actix_web::Scope {
        api::web_scope(&ctx.component())
    }

    pub async fn shut_down() {
        log::info!("Stopping payment service... It may take up to 10 seconds to send out all transactions. Hit Ctrl+C again to interrupt and shut down immediately.");
        futures::future::select(
            tokio::time::timeout(
                *PAYMENT_SHUTDOWN_TIMEOUT,
                bus::service(pay_local::BUS_ID)
                    .call(pay_local::ShutDown::new(*PAYMENT_SHUTDOWN_TIMEOUT)),
            ),
            actix_rt::signal::ctrl_c().boxed(),
        )
        .await;
        log::info!("Payment service stopped.");
    }
}

async fn release_allocations(db: Data<DbExecutor>) {
    match db
        .as_dao::<dao::AllocationDao>()
        .get_filtered(None, None, None, None, None)
        .await
    {
        Ok(allocations) => {
            if !allocations.is_empty() {
                for a in allocations {
                    release_allocation_after(db.clone(), a, None).await
                }
            }
        }
        Err(e) => {
            log::error!("Allocations release failed. Restart yagna to retry allocations release. Db error occurred: {}.", e);
        }
    }
}
