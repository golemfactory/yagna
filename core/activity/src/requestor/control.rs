use crate::common::{generate_id, PathActivity, QueryTimeout, QueryTimeoutMaxCount};
use crate::dao::AgreementDao;
use crate::db::DbExecutor;
use crate::error::Error;
use crate::requestor::get_agreement;
use crate::timeout::IntoTimeoutFuture;
use crate::{RestfulApi, ACTIVITY_SERVICE_ID, ACTIVITY_SERVICE_VERSION, NET_SERVICE_ID};
use actix_web::web;
use futures::compat::Future01CompatExt;
use futures::lock::Mutex;
use futures::prelude::*;
use serde::Deserialize;
use ya_core_model::activity::{CreateActivity, DestroyActivity, Exec, GetExecBatchResults};
use ya_model::activity::{
    ExeScriptBatch, ExeScriptCommand, ExeScriptCommandResult, ExeScriptRequest,
};

#[derive(Deserialize)]
pub struct PathActivityBatch {
    activity_id: String,
    batch_id: String,
}

pub struct RequestorControlApi {
    db_executor: Mutex<DbExecutor>,
}

impl RequestorControlApi {
    pub fn new(db_executor: Mutex<DbExecutor>) -> Self {
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

    async fn exec(
        &self,
        path: web::Path<PathActivity>,
        query: web::Query<QueryTimeout>,
        body: web::Json<ExeScriptRequest>,
    ) -> Result<String, Error> {
        let agreement = get_agreement(&self.db_executor, &path.activity_id).await?;
        let uri = Self::uri(&agreement.provider_id, "destroy_activity");
        let batch_id = generate_id();

        let msg = Exec {
            activity_id: path.activity_id.clone(),
            batch_id: batch_id.clone(),
            exe_script: parse_commands(&body)?,
            timeout: query.timeout.clone(),
        };

        gsb_send!(msg, &uri, query.timeout)?;
        Ok(batch_id)
    }

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
        .service(web::resource("/activity/{activity_id}").route(
            web::delete().to_async(impl_restful_handler!(api, destroy_activity, path, query)),
        ))
        .service(
            web::resource("/activity/{activity_id}/exec")
                .route(web::post().to_async(impl_restful_handler!(api, exec, path, query, body))),
        )
        .service(
            web::resource("/activity/{activity_id}/exec/{batch_id}").route(web::get().to_async(
                impl_restful_handler!(api, get_exec_batch_results, path, query),
            )),
        )
        .service(
            web::resource("/activity").route(web::post().to_async(impl_restful_handler!(
                api,
                create_activity,
                path,
                query
            ))),
        )
        .service(
            web::resource("/activity").route(web::get().to_async(impl_restful_handler!(
                api,
                create_activity,
                path,
                query
            ))),
        )
    }
}

fn parse_commands(request: &ExeScriptRequest) -> Result<ExeScriptBatch, Error> {
    let commands: Vec<ExeScriptCommand> = request
        .text
        .lines()
        .into_iter()
        .map(|line| match shlex::split(line) {
            Some(input) => parse_vec(input),
            None => None,
        })
        .flatten()
        .collect();

    match commands.len() {
        0 => Err(Error::BadRequest("Empty command list".to_string())),
        _ => Ok(ExeScriptBatch { commands }),
    }
}

fn parse_vec(mut input: Vec<String>) -> Option<ExeScriptCommand> {
    if !input.is_empty() {
        return None;
    }

    let command = input.remove(0);
    let params = match input.len() {
        0 => None,
        _ => Some(input),
    };
    Some(ExeScriptCommand { command, params })
}
