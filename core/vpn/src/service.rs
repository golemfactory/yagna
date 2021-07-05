use crate::network::VpnSupervisor;
use futures::lock::Mutex;
use std::sync::Arc;
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::Provider;

lazy_static::lazy_static! {
    static ref VPN_SUPERVISOR: Arc<Mutex<VpnSupervisor>> = Default::default();
}

pub struct VpnService;

impl VpnService {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(_: &Context) -> anyhow::Result<()> {
        let _vpn = VPN_SUPERVISOR.clone();
        Ok(())
    }

    pub fn rest<Context: Provider<Self, DbExecutor>>(_: &Context) -> actix_web::Scope {
        crate::requestor::web_scope(VPN_SUPERVISOR.clone())
    }
}
