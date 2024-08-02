use anyhow::anyhow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ya_core_model::driver::{driver_bus_id, Fund};
use ya_core_model::payment::local::BUS_ID;
use ya_payment::api::web_scope;
use ya_payment::config::Config;
use ya_payment::migrations;
use ya_payment::processor::PaymentProcessor;
use ya_payment::service::BindOptions;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed as bus;
use ya_service_bus::typed::Endpoint;

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
    testdir: PathBuf,

    db: DbExecutor,
    processor: Arc<PaymentProcessor>,
}

impl MockPayment {
    pub fn new(name: &str, testdir: &Path) -> Self {
        let db = Self::create_db(testdir, "payment.db").unwrap();
        let processor = Arc::new(PaymentProcessor::new(db.clone()));

        MockPayment {
            name: name.to_string(),
            testdir: testdir.to_path_buf(),
            db,
            processor,
        }
    }

    fn create_db(testdir: &Path, name: &str) -> anyhow::Result<DbExecutor> {
        let db = DbExecutor::from_data_dir(testdir, name)
            .map_err(|e| anyhow!("Failed to create db [{name:?}]. Error: {e}"))?;
        db.apply_migration(migrations::run_with_output)?;
        Ok(db)
    }

    pub async fn bind_gsb(&self) -> anyhow::Result<()> {
        log::info!("MockPayment ({}) - binding GSB", self.name);

        ya_payment::service::bind_service(
            &self.db,
            self.processor.clone(),
            BindOptions::default().run_sync_job(false),
            Arc::new(Config::from_env()?),
        );

        self.start_dummy_driver().await?;
        self.start_erc20_driver().await?;
        Ok(())
    }

    pub fn bind_rest(&self) -> actix_web::Scope {
        let db = self.db.clone();
        web_scope(&db)
    }

    pub async fn start_dummy_driver(&self) -> anyhow::Result<()> {
        dummy::PaymentDriverService::gsb(&()).await?;
        Ok(())
    }

    pub async fn start_erc20_driver(&self) -> anyhow::Result<()> {
        erc20::PaymentDriverService::gsb(self.testdir.clone()).await?;
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

    pub fn gsb_local_endpoint(&self) -> Endpoint {
        bus::service(BUS_ID)
    }
}
