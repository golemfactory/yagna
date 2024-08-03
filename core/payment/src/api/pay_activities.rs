// External crates
use crate::dao::*;
use crate::utils::*;
use actix_web::web::{get, Data, Path};
use actix_web::{HttpResponse, Scope};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .route("/payActivities", get().to(get_pay_activities))
        .route("/payActivity/{activity_id}", get().to(get_pay_activity))
}

async fn get_pay_activities(db: Data<DbExecutor>, id: Identity) -> HttpResponse {
    let node_id = id.identity;
    let dao: ActivityDao = db.as_dao();
    match dao.list(None).await {
        Ok(activities) => response::ok(activities),
        Err(e) => response::server_error(&e),
    }
}

async fn get_pay_activity(db: Data<DbExecutor>, path: Path<String>, id: Identity) -> HttpResponse {
    let node_id = id.identity;
    let activity_id = path.into_inner();
    let dao: ActivityDao = db.as_dao();
    match dao.get(activity_id, node_id).await {
        Ok(activity) => response::ok(activity),
        Err(e) => response::server_error(&e),
    }
}
