use ya_service_api_interfaces::Service;

mod rest;

pub struct HealthcheckService;

impl Service for HealthcheckService {
    type Cli = ();
}

impl HealthcheckService {
    pub fn rest<C>(_ctx: &C) -> actix_web::Scope {
        rest::web_scope()
    }
}
