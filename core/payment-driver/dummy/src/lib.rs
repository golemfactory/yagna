mod service;

pub const DRIVER_NAME: &'static str = "dummy";
pub const NETWORK_NAME: &'static str = "dummy";
pub const TOKEN_NAME: &'static str = "GLM";
pub const PLATFORM_NAME: &'static str = "dummy-glm";

pub struct PaymentDriverService;

impl PaymentDriverService {
    pub async fn gsb<Context>(_context: &Context) -> anyhow::Result<()> {
        self::service::bind_service();
        self::service::register_in_payment_service().await?;
        Ok(())
    }
}
