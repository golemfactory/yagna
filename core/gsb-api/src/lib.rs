use ya_service_api_interfaces::Provider;
use ya_service_bus::serialization::Config;

mod api;
mod services;

pub const GSB_API_PATH: &str = "/gsb-api/v1";

pub struct GsbApiService;

impl GsbApiService {
    pub async fn gsb<Context>(_: Context) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn rest<Context: Provider<Self, ()>>(ctx: &Context) -> actix_web::Scope {
        api::web_scope()
    }
}
