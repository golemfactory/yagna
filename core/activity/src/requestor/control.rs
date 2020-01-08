use crate::common::{generate_id, PathActivity, QueryTimeout, QueryTimeoutMaxCount};
use crate::dao::AgreementDao;
use crate::db::DbExecutor;
use crate::error::Error;
use crate::requestor::get_agreement;
use crate::timeout::IntoTimeoutFuture;
use crate::{RestfulApi, ACTIVITY_SERVICE_ID, ACTIVITY_SERVICE_VERSION, NET_SERVICE_ID};
use actix_web::web;
use futures::lock::Mutex;
use futures::prelude::*;
use serde::Deserialize;
use ya_core_model::activity::{CreateActivity, DestroyActivity, Exec, GetExecBatchResults};
use ya_model::activity::{ExeScriptCommand, ExeScriptCommandResult, ExeScriptRequest};

#[derive(Deserialize)]
pub struct PathActivityBatch {
    activity_id: String,
    batch_id: String,
}

pub struct RequestorControlApi {
    db_executor: Mutex<DbExecutor<Error>>,
}

impl RequestorControlApi {
    pub fn new(db_executor: Mutex<DbExecutor<Error>>) -> Self {
        Self { db_executor }
    }

    fn uri(provider_id: &str, command: &str) -> String {
        format!(
            "/{}/{}/{}/{}",
            NET_SERVICE_ID, provider_id, ACTIVITY_SERVICE_ID, command
        )
    }
}

impl RequestorControlApi {
    /// Creates new Activity based on given Agreement.
    async fn create_activity(
        &self,
        query: web::Query<QueryTimeout>,
        body: web::Json<CreateActivity>,
    ) -> Result<String, Error> {
        let agreement =
            AgreementDao::new(&self.db_executor.lock().await.conn()?).get(&body.agreement_id)?;
        let uri = Self::uri(&agreement.provider_id, "create_activity");

        gsb_send!(body.into_inner(), &uri, query.timeout)
    }

    /// Destroys given Activity.
    async fn destroy_activity(
        &self,
        path: web::Path<PathActivity>,
        query: web::Query<QueryTimeout>,
    ) -> Result<(), Error> {
        let agreement = get_agreement(&self.db_executor, &path.activity_id).await?;
        let uri = Self::uri(&agreement.provider_id, "destroy_activity");
        let msg = DestroyActivity {
            activity_id: path.activity_id.to_string(),
            agreement_id: agreement.id,
            timeout: query.timeout.clone(),
        };

        gsb_send!(msg, &uri, query.timeout)
    }

    /// Executes an ExeScript batch within a given Activity.
    async fn exec(
        &self,
        path: web::Path<PathActivity>,
        query: web::Query<QueryTimeout>,
        body: web::Json<ExeScriptRequest>,
    ) -> Result<String, Error> {
        let commands: Vec<ExeScriptCommand> =
            serde_json::from_str(&body.text).map_err(|e| Error::BadRequest(format!("{:?}", e)))?;
        let agreement = get_agreement(&self.db_executor, &path.activity_id).await?;
        let uri = Self::uri(&agreement.provider_id, "destroy_activity");
        let batch_id = generate_id();
        let msg = Exec {
            activity_id: path.activity_id.clone(),
            batch_id: batch_id.clone(),
            exe_script: commands,
            timeout: query.timeout.clone(),
        };

        gsb_send!(msg, &uri, query.timeout)?;
        Ok(batch_id)
    }

    /// Queries for ExeScript batch results.
    async fn get_exec_batch_results(
        &self,
        path: web::Path<PathActivityBatch>,
        query: web::Query<QueryTimeoutMaxCount>,
    ) -> Result<Vec<ExeScriptCommandResult>, Error> {
        let agreement = get_agreement(&self.db_executor, &path.activity_id).await?;
        let uri = Self::uri(&agreement.provider_id, "get_exec_batch_results");
        let msg = GetExecBatchResults {
            activity_id: path.activity_id.to_string(),
            batch_id: path.batch_id.to_string(),
            timeout: query.timeout.clone(),
        };

        gsb_send!(msg, &uri, query.timeout)
    }
}

impl RestfulApi for RequestorControlApi {
    fn web_scope(api: &'static Self) -> actix_web::Scope {
        web::scope(&format!(
            "/{}/v{}",
            ACTIVITY_SERVICE_ID, ACTIVITY_SERVICE_VERSION
        ))
        .service(
            web::resource("/activity/{activity_id}")
                .route(web::delete().to(impl_restful_handler!(api, destroy_activity, path, query))),
        )
        .service(
            web::resource("/activity/{activity_id}/exec")
                .route(web::post().to(impl_restful_handler!(api, exec, path, query, body))),
        )
        .service(
            web::resource("/activity/{activity_id}/exec/{batch_id}").route(web::get().to(
                impl_restful_handler!(api, get_exec_batch_results, path, query),
            )),
        )
        .service(
            web::resource("/activity").route(web::post().to(impl_restful_handler!(
                api,
                create_activity,
                path,
                query
            ))),
        )
        .service(
            web::resource("/activity").route(web::get().to(impl_restful_handler!(
                api,
                create_activity,
                path,
                query
            ))),
        )
    }
}
