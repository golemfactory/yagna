use actix_web::Scope;

use ya_persistence::executor::DbExecutor;
use ya_service_api_web::scope::ExtendableScope;

pub fn web_scope(db: &DbExecutor) -> Scope {
    actix_web::web::scope(crate::ACTIVITY_API_PATH)
        .data(db.clone())
        .extend(common::extend_web_scope)
        .extend(crate::provider::extend_web_scope)
        .extend(crate::requestor::control::extend_web_scope)
        .extend(crate::requestor::state::extend_web_scope)
}

/// Common operations for both sides: Provider and Requestor
mod common {
    use actix_web::{web, Responder};
    use ya_service_bus::{timeout::IntoTimeoutFuture, RpcEndpoint};

    use ya_core_model::{activity, Role};
    use ya_persistence::executor::DbExecutor;
    use ya_service_api_web::middleware::Identity;

    use crate::common::*;

    pub fn extend_web_scope(scope: actix_web::Scope) -> actix_web::Scope {
        scope
            // .service(get_activities_web)
            .service(get_activity_state_web)
            .service(get_activity_usage_web)
    }

    // TODO this endpoint needs authorization via Identity, otherwise is vulnerable for attacks.
    // #[actix_web::get("/activity")]
    // async fn get_activities_web(db: web::Data<DbExecutor>) -> impl Responder {
    //     log::debug!("get_activities_web");
    //     get_activities(&db).await.map(web::Json)
    // }
    #[actix_web::get("/activity/{activity_id}/state")]
    async fn get_activity_state_web(
        db: web::Data<DbExecutor>,
        path: web::Path<PathActivity>,
        query: web::Query<QueryTimeout>,
        id: Identity,
    ) -> impl Responder {
        log::debug!("get_activity_state_web");

        // check if caller is the Provider
        if authorize_activity_executor(&db, id.identity, &path.activity_id, Role::Provider)
            .await
            .is_ok()
        {
            log::trace!("get_activity_state_web: I'm the provider");
            return get_persisted_state(&db, &path.activity_id)
                .await
                .map(web::Json);
        }

        log::trace!("get_activity_state_web: Not provider, maybe requestor?");

        // check if caller is the Requestor
        authorize_activity_initiator(&db, id.identity, &path.activity_id, Role::Requestor).await?;

        log::trace!("get_activity_state_web: I'm the requestor");

        // Return locally persisted usage if activity has been already terminated or terminating
        let state = get_persisted_state(&db, &path.activity_id).await?;
        if !state.alive() {
            log::trace!("get_activity_state_web: got persisted state");
            return Ok(web::Json(state));
        }

        // Retrieve and persist activity state
        let agreement = get_activity_agreement(&db, &path.activity_id, Role::Requestor).await?;
        let provider_service = agreement_provider_service(&id, &agreement)?;
        let state = provider_service
            .send(activity::GetState {
                activity_id: path.activity_id.to_string(),
                timeout: query.timeout.clone(),
            })
            .timeout(timeout_margin(query.timeout))
            .await???;

        set_persisted_state(&db, &path.activity_id, state)
            .await
            .map(web::Json)
    }

    #[actix_web::get("/activity/{activity_id}/usage")]
    async fn get_activity_usage_web(
        db: web::Data<DbExecutor>,
        path: web::Path<PathActivity>,
        query: web::Query<QueryTimeout>,
        id: Identity,
    ) -> impl Responder {
        // check if caller is the Provider
        if authorize_activity_executor(&db, id.identity, &path.activity_id, Role::Provider)
            .await
            .is_ok()
        {
            return get_persisted_usage(&db, &path.activity_id)
                .await
                .map(web::Json);
        }

        // check if caller is the Requestor
        authorize_activity_initiator(&db, id.identity, &path.activity_id, Role::Requestor).await?;

        // Return locally persisted usage if activity has been already terminated or terminating
        let state = get_persisted_state(&db, &path.activity_id).await?;
        if !state.alive() {
            return get_persisted_usage(&db, &path.activity_id)
                .await
                .map(web::Json);
        }

        // Retrieve and persist activity usage
        let agreement = get_activity_agreement(&db, &path.activity_id, Role::Requestor).await?;
        let provider_service = agreement_provider_service(&id, &agreement)?;
        let usage = provider_service
            .send(activity::GetUsage {
                activity_id: path.activity_id.to_string(),
                timeout: query.timeout.clone(),
            })
            .timeout(timeout_margin(query.timeout))
            .await???;

        set_persisted_usage(&db, &path.activity_id, usage)
            .await
            .map(web::Json)
    }
}
