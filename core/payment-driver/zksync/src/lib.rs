// #[macro_use]
// extern crate diesel;
#[macro_use]
extern crate log;

mod service;
pub mod zksync;
mod utils;

pub const PLATFORM_NAME: &'static str = "ZK-NGNT";
pub const DRIVER_NAME: &'static str = "zksync";

pub struct PaymentDriverService;

impl PaymentDriverService {
    pub async fn gsb<Context>(_context: &Context) -> anyhow::Result<()> {
        self::service::bind_service();
        self::service::subscribe_to_identity_events().await;
        Ok(())
    }
}
