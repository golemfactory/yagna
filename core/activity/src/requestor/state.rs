use crate::common::{PathActivity, QueryTimeout};
use crate::db::DbExecutor;
use crate::error::Error;
use crate::requestor::get_agreement;
use crate::timeout::IntoTimeoutFuture;
use crate::{RestfulApi, ACTIVITY_SERVICE_ID, ACTIVITY_SERVICE_VERSION, NET_SERVICE_ID};
use actix_web::web;
use futures::lock::Mutex;
use futures::prelude::*;
use ya_core_model::activity::{GetActivityState, GetActivityUsage, GetRunningCommand};
use ya_model::activity::{ActivityState, ActivityUsage, ExeScriptCommandState};

pub struct RequestorStateApi {
    db_executor: Mutex<DbExecutor<Error>>,
}

impl RequestorStateApi {
    pub fn new(db_executor: Mutex<DbExecutor<Error>>) -> Self {
        Self { db_executor }
    }

    fn uri(provider_id: &str, cmd: &str) -> String {
        format!(
            "/{}/{}/{}/{}",
            NET_SERVICE_ID, provider_id, ACTIVITY_SERVICE_ID, cmd
        )
    }
}

impl RequestorStateApi {
    /// Get state of specified Activity.
    pub async fn get_activity_state(
        &self,
        path: web::Path<PathActivity>,
        query: web::Query<QueryTimeout>,
    ) -> Result<ActivityState, Error> {
        let agreement = get_agreement(&self.db_executor, &path.activity_id).await?;
        let uri = Self::uri(&agreement.provider_id, "get_activity_state");
        let msg = GetActivityState {
            activity_id: path.activity_id.to_string(),
            timeout: query.timeout.clone(),
        };

        gsb_send!(msg, &uri, query.timeout)
    }

    /// Get usage of specified Activity.
    pub async fn get_activity_usage(
        &self,
        path: web::Path<PathActivity>,
        query: web::Query<QueryTimeout>,
    ) -> Result<ActivityUsage, Error> {
        let agreement = get_agreement(&self.db_executor, &path.activity_id).await?;
        let uri = Self::uri(&agreement.provider_id, "get_activity_usage");
        let msg = GetActivityUsage {
            activity_id: path.activity_id.to_string(),
            timeout: query.timeout.clone(),
        };

        gsb_send!(msg, &uri, query.timeout)
    }

    /// Get running command for a specified Activity.
    pub async fn get_running_command(
        &self,
        path: web::Path<PathActivity>,
        query: web::Query<QueryTimeout>,
    ) -> Result<ExeScriptCommandState, Error> {
        let agreement = get_agreement(&self.db_executor, &path.activity_id).await?;
        let uri = Self::uri(&agreement.provider_id, "get_running_command");
        let msg = GetRunningCommand {
            activity_id: path.activity_id.to_string(),
            timeout: query.timeout.clone(),
        };

        gsb_send!(msg, &uri, query.timeout)
    }
}

impl RestfulApi for RequestorStateApi {
    fn web_scope(api: &'static Self) -> actix_web::Scope {
        web::scope(&format!(
            "/{}/v{}",
            ACTIVITY_SERVICE_ID, ACTIVITY_SERVICE_VERSION
        ))
        .service(
            web::resource("/activity/{activity_id}/state")
                .route(web::get().to(impl_restful_handler!(api, get_activity_state, path, query))),
        )
        .service(
            web::resource("/activity/{activity_id}/usage")
                .route(web::get().to(impl_restful_handler!(api, get_activity_usage, path, query))),
        )
        .service(
            web::resource("/activity/{activity_id}/command")
                .route(web::get().to(impl_restful_handler!(api, get_running_command, path, query))),
        )
    }
}
