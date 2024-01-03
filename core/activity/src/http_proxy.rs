use actix_web::{web, Responder};

use ya_client_model::market::Role;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::timeout::IntoTimeoutFuture;

use crate::common::*;

pub fn extend_web_scope(scope: actix_web::Scope) -> actix_web::Scope {
    scope.service(proxy_http_request)
}

#[actix_web::get("/activity/{activity_id}/proxy_http_request")]
async fn proxy_http_request(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
    id: Identity,
) -> impl Responder {
    // check if caller is the Provider
    authorize_activity_executor(&db, id.identity, &path.activity_id, Role::Provider).await?;

    ()
}
