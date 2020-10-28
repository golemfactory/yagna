/*
    The service that binds this payment driver into yagna via GSB.
*/

// Workspace uses
use ya_payment_driver::bus;

// Local uses
use crate::driver::ZksyncDriver;

pub struct ZksyncService;

impl ZksyncService {
    pub async fn gsb<Context>(_context: &Context) -> anyhow::Result<()> {
        log::debug!("Connecting ZksyncService to gsb...");

        let driver = ZksyncDriver::new();
        driver.load_active_accounts().await;
        bus::bind_service(driver).await;

        log::info!("Succesfully connected ZksyncService to gsb.");
        Ok(())
    }
}
