mod service;

pub const DRIVER_NAME: &str = "dummy";
pub const NETWORK_NAME: &str = "dummy";
pub const TOKEN_NAME: &str = "GLM";
pub const PLATFORM_NAME: &str = "dummy-glm";

pub struct PaymentDriverService;

impl PaymentDriverService {
    pub async fn gsb<Context>(_context: &Context) -> anyhow::Result<()> {
        self::service::bind_service();
        self::service::register_in_payment_service().await?;
        Ok(())
    }
}
