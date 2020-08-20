// #[macro_use]
// extern crate diesel;

mod service;

pub const PLATFORM_NAME: &'static str = "ZK-NGNT";
pub const DRIVER_NAME: &'static str = "zksync";

pub struct PaymentDriverService;

impl PaymentDriverService {
    pub async fn gsb<Context>(_context: &Context) -> anyhow::Result<()> {
        self::service::bind_service();
        Ok(())
    }
}
