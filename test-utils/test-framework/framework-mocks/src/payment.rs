use anyhow::anyhow;
use chrono::{DateTime, TimeZone, Utc};

use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ya_client::payment::PaymentApi;
use ya_client_model::payment::{Acceptance, Allocation, DebitNote, Invoice, Payment};
use ya_core_model::driver::{driver_bus_id, Fund};
use ya_core_model::payment::local::{
    NetworkName, ProcessBatchCycleResponse, ProcessBatchCycleSet, BUS_ID,
};
use ya_core_model::payment::public;
use ya_core_model::NodeId;
use ya_payment::api::web_scope;
use ya_payment::config::Config;
use ya_payment::migrations;
use ya_payment::processor::PaymentProcessor;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed as bus;
use ya_service_bus::typed::Endpoint;

use ya_dummy_driver as dummy;
use ya_erc20_driver as erc20;
use ya_payment::alloc_release_task::AllocationReleaseTasks;

pub mod fake_payment;

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
pub struct RealPayment {
    name: String,
    testdir: PathBuf,

    db: DbExecutor,
    processor: Arc<PaymentProcessor>,

    config: Arc<Config>,

    allocation_release_tasks: AllocationReleaseTasks,
}

impl RealPayment {
    pub fn new(name: &str, testdir: &Path) -> Self {
        let db = Self::create_db(testdir, "payment.db").unwrap();
        let processor = Arc::new(PaymentProcessor::new(
            db.clone(),
            AllocationReleaseTasks::new(),
        ));
        let config = Config::from_env().unwrap().run_sync_job(false);

        RealPayment {
            name: name.to_string(),
            testdir: testdir.to_path_buf(),
            db,
            processor,
            config: Arc::new(config),
            allocation_release_tasks: AllocationReleaseTasks::new(),
        }
    }

    pub fn with_config(mut self, config: Option<Config>) -> Self {
        self.config = Arc::new(config.unwrap_or(Config::from_env().unwrap()));
        self
    }

    fn create_db(testdir: &Path, name: &str) -> anyhow::Result<DbExecutor> {
        let db = DbExecutor::from_data_dir(testdir, name)
            .map_err(|e| anyhow!("Failed to create db [{name:?}]. Error: {e}"))?;
        db.apply_migration(migrations::run_with_output)?;
        Ok(db)
    }

    pub async fn bind_gsb(&self) -> anyhow::Result<()> {
        log::info!("RealPayment ({}) - binding GSB", self.name);

        ya_payment::service::bind_service(&self.db, self.processor.clone(), self.config.clone())
            .await?;
        self.processor.process_post_migration_jobs().await?;

        self.start_dummy_driver().await?;
        self.start_erc20_driver().await?;
        Ok(())
    }

    pub fn bind_rest(&self) -> actix_web::Scope {
        let db = self.db.clone();
        web_scope(&db, self.allocation_release_tasks.clone())
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
                false,
            ))
            .await??;
        Ok(())
    }

    pub async fn set_payment_processing_interval(
        &self,
        driver: Driver,
        network: NetworkName,
        node_id: NodeId,
        interval: Duration,
    ) -> anyhow::Result<ProcessBatchCycleResponse> {
        Ok(self
            .gsb_local_endpoint()
            .call(ProcessBatchCycleSet {
                node_id,
                interval: Some(interval),
                platform: format!("{}-{}-{}", driver, network, network.get_token()).to_lowercase(),
                cron: None,
                next_update: None,
                safe_payout: None,
            })
            .await??)
    }

    pub async fn set_all_payment_processing_intervals(
        &self,
        node_id: NodeId,
        interval: Duration,
    ) -> anyhow::Result<()> {
        let drivers = vec![Driver::Dummy, Driver::Erc20];
        let networks = vec![
            NetworkName::Holesky,
            NetworkName::Amoy,
            NetworkName::Mumbai,
            NetworkName::Rinkeby,
            NetworkName::Goerli,
            NetworkName::Mainnet,
            NetworkName::Polygon,
        ];

        for driver in drivers {
            for network in &networks {
                self.set_payment_processing_interval(
                    driver.clone(),
                    network.clone(),
                    node_id,
                    interval,
                )
                .await?;
            }
        }
        Ok(())
    }

    pub fn gsb_local_endpoint(&self) -> Endpoint {
        bus::service(BUS_ID)
    }

    pub fn gsb_public_endpoint(&self) -> Endpoint {
        bus::service(public::BUS_ID)
    }
}

#[async_trait::async_trait(?Send)]
pub trait PaymentRestExt {
    async fn wait_for_payment<Tz>(
        &self,
        after_timestamp: Option<&DateTime<Tz>>,
        timeout: Duration,
        max_events: Option<u32>,
        app_session_id: Option<String>,
    ) -> anyhow::Result<Vec<Payment>>
    where
        Tz: TimeZone,
        Tz::Offset: Display;

    async fn wait_for_invoice_payment<Tz>(
        &self,
        invoice_id: &str,
        timeout: Duration,
        after_timestamp: Option<DateTime<Tz>>,
    ) -> anyhow::Result<Vec<Payment>>
    where
        Tz: TimeZone,
        Tz::Offset: Display;

    async fn simple_accept_invoice(
        &self,
        invoice: &Invoice,
        allocation: &Allocation,
    ) -> anyhow::Result<()>;

    async fn simple_accept_debit_note(
        &self,
        debit_note: &DebitNote,
        allocation: &Allocation,
    ) -> anyhow::Result<()>;
}

#[async_trait::async_trait(?Send)]
impl PaymentRestExt for PaymentApi {
    async fn wait_for_payment<Tz>(
        &self,
        after_timestamp: Option<&DateTime<Tz>>,
        timeout: Duration,
        max_events: Option<u32>,
        app_session_id: Option<String>,
    ) -> anyhow::Result<Vec<Payment>>
    where
        Tz: TimeZone,
        Tz::Offset: Display,
    {
        let start = Utc::now();
        // Workaround: Can't pass timeout to `get_payments`, because serde_urlencoded is unable to deserialize it.
        // https://github.com/nox/serde_urlencoded/issues/33
        while start + timeout > Utc::now() {
            let payments = self
                .get_payments(after_timestamp, None, max_events, app_session_id.clone())
                .await?;

            if !payments.is_empty() {
                return Ok(payments);
            }
        }
        Err(anyhow!("Timeout {timeout:?} waiting for payments."))
    }

    async fn wait_for_invoice_payment<Tz>(
        &self,
        invoice_id: &str,
        timeout: Duration,
        after_timestamp: Option<DateTime<Tz>>,
    ) -> anyhow::Result<Vec<Payment>>
    where
        Tz: TimeZone,
        Tz::Offset: Display,
    {
        let start = Utc::now();
        while start + timeout > Utc::now() {
            let payments = self
                .get_payments_for_invoice(invoice_id, after_timestamp.clone(), None)
                .await?;

            if !payments.is_empty() {
                return Ok(payments);
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        Err(anyhow!("Timeout {timeout:?} waiting for payments."))
    }

    async fn simple_accept_invoice(
        &self,
        invoice: &Invoice,
        allocation: &Allocation,
    ) -> anyhow::Result<()> {
        Ok(self
            .accept_invoice(
                &invoice.invoice_id,
                &Acceptance {
                    total_amount_accepted: invoice.amount.clone(),
                    allocation_id: allocation.allocation_id.to_string(),
                },
            )
            .await?)
    }

    async fn simple_accept_debit_note(
        &self,
        debit_note: &DebitNote,
        allocation: &Allocation,
    ) -> anyhow::Result<()> {
        Ok(self
            .accept_debit_note(
                &debit_note.debit_note_id,
                &Acceptance {
                    total_amount_accepted: debit_note.total_amount_due.clone(),
                    allocation_id: allocation.allocation_id.to_string(),
                },
            )
            .await?)
    }
}
