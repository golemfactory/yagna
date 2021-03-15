use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::Provider;

pub struct VpnService;

impl VpnService {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(_: &Context) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn rest<Context: Provider<Self, DbExecutor>>(_: &Context) -> actix_web::Scope {
        crate::requestor::web_scope()
    }
}
