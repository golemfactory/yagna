pub mod all_cors;
pub mod appkey_cors;

use crate::ServiceContext;
use std::sync::Arc;
use ya_service_api_web::middleware::cors::AppKeyCors;

pub struct CreateServerArgs {
    pub cors: AppKeyCors,
    pub context: ServiceContext,
    pub number_of_workers: usize,
    pub count_started: Arc<std::sync::atomic::AtomicUsize>,
    pub rest_address: String,
    pub max_rest_timeout: u64,
    pub api_host_port: String,
}
