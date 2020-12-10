//! Provider side operations
use actix_web::{web, Responder};

use ya_client_model::activity::{ActivityState, ProviderEvent};
use ya_core_model::market::Role;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::timeout::IntoTimeoutFuture;

use crate::common::{
    authorize_activity_executor, set_persisted_state, PathActivity, QueryTimeoutMaxEvents,
};
use crate::dao::EventDao;
use crate::error::Error;

pub mod service;

pub fn extend_web_scope(scope: actix_web::Scope) -> actix_web::Scope {
    scope
        .service(get_events_web)
        .service(set_activity_state_web)
}

#[actix_web::put("/activity/{activity_id}/state")]
async fn set_activity_state_web(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    state: web::Json<ActivityState>,
    id: Identity,
) -> impl Responder {
    log::debug!("set_activity_state_web {:?}", state);
    authorize_activity_executor(&db, id.identity, &path.activity_id, Role::Provider).await?;

    set_persisted_state(&db, &path.activity_id, state.into_inner())
        .await
        .map(|_| web::Json(()))
}

/// Fetch Requestor command events.
#[actix_web::get("/events")]
async fn get_events_web(
    db: web::Data<DbExecutor>,
    query: web::Query<QueryTimeoutMaxEvents>,
    id: Identity,
) -> impl Responder {
    log::trace!("getting events {:?}", query);
    let events = db
        .as_dao::<EventDao>()
        .get_events_wait(id.identity, query.max_events)
        .timeout(query.timeout)
        .await??
        .into_iter()
        .collect::<Vec<ProviderEvent>>();

    Ok::<_, Error>(web::Json(events))
}
