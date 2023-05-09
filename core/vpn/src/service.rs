use crate::network::{VpnSupervisorRef};
use actix_web::web;
use tokio::sync::RwLock;
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::Provider;

pub struct VpnService;

impl VpnService {

    pub fn rest<Context: Provider<Self, DbExecutor>>(_: &Context) -> actix_web::Scope {

        lazy_static::lazy_static! {
            static ref VPN_SUPERVISOR: web::Data<VpnSupervisorRef> = web::Data::new(RwLock::new(Default::default()));
        }

        crate::requestor::web_scope(VPN_SUPERVISOR.clone())
    }
}
