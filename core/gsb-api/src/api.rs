use crate::services;
use actix_web::web::{service, Data};
use actix_web::Scope;

use ya_service_api_web::scope::ExtendableScope;

pub fn web_scope() -> Scope {
    actix_web::web::scope(crate::GSB_API_PATH).extend(services::extend_web_scope)
}
