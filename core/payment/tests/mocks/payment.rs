#![allow(dead_code)]

use actix_web::web::Data;
use actix_web::{middleware, App, HttpServer};
use anyhow::anyhow;
use std::sync::Arc;

use ya_core_model::driver::{driver_bus_id, Fund};
use ya_framework_basic::async_drop::DroppableTestContext;
use ya_payment::api::web_scope;
use ya_payment::migrations;
use ya_payment::processor::PaymentProcessor;
use ya_payment::service::BindOptions;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed as bus;

use ya_dummy_driver as dummy;
use ya_erc20_driver as erc20;

#[derive(Clone, Debug, derive_more::Display)]
pub enum Driver {
    #[display(fmt = "dummy")]
    Dummy,
    #[display(fmt = "erc20")]
    Erc20,
}

impl Driver {
    pub fn gsb_name(&self) -> String {
        match self {
            Driver::Dummy => dummy::DRIVER_NAME.to_string(),
            Driver::Erc20 => erc20::DRIVER_NAME.to_string(),
        }
    }
}

#[derive(Clone)]
pub struct MockPayment {
    name: String,
    db: DbExecutor,
    processor: Arc<PaymentProcessor>,
}

impl MockPayment {
    pub fn new(name: &str) -> Self {
        let db = Self::create_db(&format!("{name}.payment.db")).unwrap();
        let processor = Arc::new(PaymentProcessor::new(db.clone()));

        MockPayment {
            name: name.to_string(),
            db,
            processor,
        }
    }

    fn create_db(name: &str) -> anyhow::Result<DbExecutor> {
        let db = DbExecutor::in_memory(name)
            .map_err(|e| anyhow!("Failed to create in memory db [{name:?}]. Error: {e}"))?;
        db.apply_migration(migrations::run_with_output)?;
        Ok(db)
    }

    pub async fn bind_gsb(&self) -> anyhow::Result<()> {
        log::info!("MockPayment ({}) - binding GSB", self.name);

        ya_payment::service::bind_service(
            &self.db,
            self.processor.clone(),
            BindOptions::default().run_sync_job(false),
        );
        Ok(())
    }

    pub async fn start_server(
        &self,
        ctx: &mut DroppableTestContext,
        address: &str,
    ) -> anyhow::Result<()> {
        let db = self.db.clone();
        let srv = HttpServer::new(move || {
            App::new()
                .wrap(middleware::Logger::default())
                .app_data(Data::new(db.clone()))
                .service(web_scope(&db))
        })
        .bind(address)?
        .run();

        ctx.register(srv.handle());
        tokio::task::spawn_local(async move { anyhow::Ok(srv.await?) });

        Ok(())
    }

    pub async fn start_dummy_driver() -> anyhow::Result<()> {
        dummy::PaymentDriverService::gsb(&()).await?;
        Ok(())
    }

    pub async fn start_erc20_driver() -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn fund_account(&self, driver: Driver, address: &str) -> anyhow::Result<()> {
        bus::service(driver_bus_id(driver.gsb_name()))
            .call(Fund::new(
                address.to_string(),
                Some("holesky".to_string()),
                None,
            ))
            .await??;
        Ok(())
    }
}
