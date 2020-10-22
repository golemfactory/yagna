// #[macro_use]
// extern crate diesel;
#[macro_use]
extern crate log;

mod faucet;
mod service;
mod utils;
pub mod zksync;

pub const PLATFORM_NAME: &'static str = "ZK-NGNT";
pub const DRIVER_NAME: &'static str = "zksync";
pub const ZKSYNC_TOKEN_NAME: &'static str = "GNT";

pub struct PaymentDriverService;

impl PaymentDriverService {
    pub async fn gsb<Context>(_context: &Context) -> anyhow::Result<()> {
        self::service::bind_service();
        self::service::subscribe_to_identity_events().await;
        Ok(())
    }
}
