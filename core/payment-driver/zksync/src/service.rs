/*
    The service that binds this payment driver into yagna via GSB.
*/

// Workspace uses
use ya_payment_driver::{bus, dao::{init, DbExecutor}, model::GenericError};
use ya_service_api_interfaces::Provider;

// Local uses
use crate::driver::ZksyncDriver;

pub struct ZksyncService;

impl ZksyncService {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(context: &Context) -> anyhow::Result<()> {
        log::debug!("Connecting ZksyncService to gsb...");

        // TODO: Read and validate env
        log::debug!("Environment variables validated");

        // Init database
        let db: DbExecutor = context.component();
        init(&db).await.map_err(GenericError::new)?;
        log::debug!("Database initialised");

        // Load driver
        let driver = ZksyncDriver::new();
        bus::bind_service(&db, driver).await;
        log::debug!("Driver loaded");

        // Start cron
        // cron::start(db.clone());
        log::debug!("Cron started");

        log::info!("Succesfully connected ZksyncService to gsb.");
        Ok(())
    }
}
