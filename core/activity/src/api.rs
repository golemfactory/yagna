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

    use ya_core_model::activity;
    use ya_net::TryRemoteEndpoint;
    use ya_persistence::executor::DbExecutor;
    use ya_service_api_web::middleware::Identity;
    use ya_service_bus::{timeout::IntoTimeoutFuture, RpcEndpoint};

    use crate::common::{
        authorize_activity_executor, authorize_activity_initiator, get_activity_agreement,
        get_persisted_state, get_persisted_usage, set_persisted_state, set_persisted_usage,
        PathActivity, QueryTimeout,
    };

    pub fn extend_web_scope(scope: actix_web::Scope) -> actix_web::Scope {
        scope
            .service(get_activity_state_web)
            .service(get_activity_usage_web)
    }

    #[actix_web::get("/activity/{activity_id}/state")]
    async fn get_activity_state_web(
        db: web::Data<DbExecutor>,
        path: web::Path<PathActivity>,
        query: web::Query<QueryTimeout>,
        id: Identity,
    ) -> impl Responder {
        // check if caller is the Provider
        if authorize_activity_executor(&db, id.identity, &path.activity_id)
            .await
            .is_ok()
        {
            return get_persisted_state(&db, &path.activity_id)
                .await
                .map(web::Json);
        }

        // check if caller is the Requestor
        authorize_activity_initiator(&db, id.identity, &path.activity_id).await?;

        // Return locally persisted usage if activity has been already terminated or terminating
        let state = get_persisted_state(&db, &path.activity_id).await?;
        if !state.alive() {
            return Ok(web::Json(state));
        }

        // Retrieve and persist activity state
        let agreement = get_activity_agreement(&db, &path.activity_id).await?;
        let provider_service = agreement.provider_id()?.try_service(activity::BUS_ID)?;
        let state = provider_service
            .send(activity::GetState {
                activity_id: path.activity_id.to_string(),
                timeout: query.timeout.clone(),
            })
            .timeout(query.timeout)
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
        if authorize_activity_executor(&db, id.identity, &path.activity_id)
            .await
            .is_ok()
        {
            return get_persisted_usage(&db, &path.activity_id)
                .await
                .map(web::Json);
        }

        // check if caller is the Requestor
        authorize_activity_initiator(&db, id.identity, &path.activity_id).await?;

        // Return locally persisted usage if activity has been already terminated or terminating
        if get_persisted_state(&db, &path.activity_id).await?.alive() {
            return get_persisted_usage(&db, &path.activity_id)
                .await
                .map(web::Json);
        }

        // Retrieve and persist activity usage
        let agreement = get_activity_agreement(&db, &path.activity_id).await?;
        let provider_service = agreement.provider_id()?.try_service(activity::BUS_ID)?;
        let usage = provider_service
            .send(activity::GetUsage {
                activity_id: path.activity_id.to_string(),
                timeout: query.timeout.clone(),
            })
            .timeout(query.timeout)
            .await???;

        set_persisted_usage(&db, &path.activity_id, usage)
            .await
            .map(web::Json)
    }
}
