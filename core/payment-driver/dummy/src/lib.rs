mod service;

pub const PLATFORM_NAME: &'static str = "DUMMY";
pub const DRIVER_NAME: &'static str = "dummy";

pub struct PaymentDriverService;

impl PaymentDriverService {
    pub async fn gsb<Context>(_context: &Context) -> anyhow::Result<()> {
        self::service::bind_service();
        Ok(())
    }
}
