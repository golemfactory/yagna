pub mod appkey_cors;

use crate::ServiceContext;
use ya_service_api_web::middleware::cors::AppKeyCors;

pub struct CreateServerArgs {
    pub cors: AppKeyCors,
    pub cors_on_auth_failure: bool,
    pub context: ServiceContext,
    pub number_of_workers: usize,
    pub rest_address: String,
    pub max_rest_timeout: u64,
    pub api_host_port: String,
}
