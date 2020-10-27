/*
    The service that binds this payment driver into yagna via GSB.
*/

// Workspace uses
use ya_payment_driver::{account::Accounts, bus};

// Local uses
use crate::driver::ZksyncDriver;

pub struct ZksyncService;

impl ZksyncService {
    pub async fn gsb<Context>(_context: &Context) -> anyhow::Result<()> {
        log::debug!("Connecting ZksyncService to gsb...");

        let accounts = Accounts::new_rc();
        let driver = ZksyncDriver::new_arc(accounts);

        bus::bind_service(driver.clone());
        bus::subscribe_to_identity_events(driver).await;

        log::info!("Succesfully connected ZksyncService to gsb.");
        Ok(())
    }
}
