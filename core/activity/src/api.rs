use actix_web::Scope;

use ya_persistence::executor::DbExecutor;

pub fn web_scope(db: &DbExecutor) -> Scope {
    let mut activity = actix_web::web::scope(crate::ACTIVITY_API).data(db.clone());
    activity = crate::provider::extend_web_scope(activity);
    activity = crate::requestor::control::extend_web_scope(activity);
    activity = crate::requestor::state::extend_web_scope(activity);
    activity
}
