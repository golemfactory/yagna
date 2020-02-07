use actix_web::Scope;
use ya_model::activity::ACTIVITY_API_PATH;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::scope::ExtendableScope;

pub fn web_scope(db: &DbExecutor) -> Scope {
    actix_web::web::scope(ACTIVITY_API_PATH)
        .data(db.clone())
        .extend(crate::provider::extend_web_scope)
        .extend(crate::requestor::control::extend_web_scope)
        .extend(crate::requestor::state::extend_web_scope)
}
