use actix_web::{web, Responder};
use futures::StreamExt;
use serde::Deserialize;

use ya_client_model::activity::{
    ActivityState, CreateActivityRequest, CreateActivityResult, Credentials, ExeScriptCommand,
    ExeScriptRequest, SgxCredentials, State,
};
use ya_core_model::activity;
use ya_net::{self as net, RemoteEndpoint};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::{timeout::IntoTimeoutFuture, RpcEndpoint};

use crate::common::{
    agreement_provider_service, authorize_activity_initiator, authorize_agreement_initiator,
    generate_id, get_activity_agreement, get_agreement, set_persisted_state, PathActivity,
    QueryTimeout, QueryTimeoutCommandIndex,
};
use crate::dao::ActivityDao;
use crate::{error::Error, Result};

pub fn extend_web_scope(scope: actix_web::Scope) -> actix_web::Scope {
    scope
        .service(create_activity)
        .service(destroy_activity)
        .service(exec)
        .service(get_batch_results)
        .service(encrypted)
}

/// Creates new Activity based on given Agreement.
#[actix_web::post("/activity")]
async fn create_activity(
    db: web::Data<DbExecutor>,
    query: web::Query<QueryTimeout>,
    body: web::Json<CreateActivityRequest>,
    id: Identity,
) -> impl Responder {
    let agreement_id = &body.agreement_id;
    authorize_agreement_initiator(id.identity, agreement_id).await?;

    let agreement = get_agreement(&agreement_id).await?;
    log::trace!("agreement: {:#?}", agreement);

    let provider_id = agreement.provider_id()?.parse()?;
    let msg = activity::Create {
        provider_id,
        agreement_id: agreement_id.clone(),
        timeout: query.timeout.clone(),
        requestor_pub_key: body
            .requestor_pub_key
            .as_ref()
            .map(hex::decode)
            .transpose()
            .map_err(|e| Error::BadRequest(format!("Invalid requestor pub key: {}", e)))?,
    };

    let create_resp = net::from(id.identity)
        .to(provider_id)
        .service(activity::BUS_ID)
        .send(msg)
        .timeout(query.timeout)
        .await???;

    log::debug!("activity created: {}, inserting", create_resp.activity_id);
    db.as_dao::<ActivityDao>()
        .create_if_not_exists(&create_resp.activity_id, agreement_id)
        .await?;

    let create_result = CreateActivityResult {
        activity_id: create_resp.activity_id,
        credentials: create_resp
            .credentials
            .map(convert_credentials)
            .transpose()?,
    };

    Ok::<_, Error>(web::Json(create_result))
}

/// Destroys given Activity.
#[actix_web::delete("/activity/{activity_id}")]
async fn destroy_activity(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
    id: Identity,
) -> impl Responder {
    authorize_activity_initiator(&db, id.identity, &path.activity_id).await?;

    let agreement = get_activity_agreement(&db, &path.activity_id).await?;
    let msg = activity::Destroy {
        activity_id: path.activity_id.to_string(),
        agreement_id: agreement.agreement_id.clone(),
        timeout: query.timeout.clone(),
    };
    agreement_provider_service(&id, &agreement)?
        .send(msg)
        .timeout(query.timeout)
        .await???;

    set_persisted_state(
        &db,
        &path.activity_id,
        ActivityState {
            state: State::Terminated.into(),
            reason: None,
            error_message: None,
        },
    )
    .await
    .map(|_| web::Json(()))
}

/// Executes an ExeScript batch within a given Activity.
#[actix_web::post("/activity/{activity_id}/exec")]
async fn exec(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
    body: web::Json<ExeScriptRequest>,
    id: Identity,
) -> impl Responder {
    authorize_activity_initiator(&db, id.identity, &path.activity_id).await?;

    let commands: Vec<ExeScriptCommand> =
        serde_json::from_str(&body.text).map_err(|e| Error::BadRequest(format!("{:?}", e)))?;
    let agreement = get_activity_agreement(&db, &path.activity_id).await?;
    let batch_id = generate_id();
    let msg = activity::Exec {
        activity_id: path.activity_id.clone(),
        batch_id: batch_id.clone(),
        exe_script: commands,
        timeout: query.timeout.clone(),
    };

    ya_net::from(id.identity)
        .to(agreement.provider_id()?.parse()?)
        .service(&activity::exeunit::bus_id(&path.activity_id))
        .send(msg)
        .timeout(query.timeout)
        .await???;

    Ok::<_, Error>(web::Json(batch_id))
}

/// Queries for ExeScript batch results.
#[actix_web::get("/activity/{activity_id}/exec/{batch_id}")]
async fn get_batch_results(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivityBatch>,
    query: web::Query<QueryTimeoutCommandIndex>,
    id: Identity,
) -> impl Responder {
    authorize_activity_initiator(&db, id.identity, &path.activity_id).await?;

    let agreement = get_activity_agreement(&db, &path.activity_id).await?;
    let msg = activity::GetExecBatchResults {
        activity_id: path.activity_id.to_string(),
        batch_id: path.batch_id.to_string(),
        timeout: query.timeout,
        command_index: query.command_index,
    };

    let results = ya_net::from(id.identity)
        .to(agreement.provider_id()?.parse()?)
        .service(&activity::exeunit::bus_id(&path.activity_id))
        .send(msg)
        .timeout(query.timeout)
        .await???;

    Ok::<_, Error>(web::Json(results))
}

/// Forwards an encrypted ExeUnit call.
#[actix_web::post("/activity/{activity_id}/encrypted")]
async fn encrypted(
    db: web::Data<DbExecutor>,
    path: web::Path<PathActivity>,
    query: web::Query<QueryTimeout>,
    mut body: web::Payload,
    id: Identity,
) -> impl Responder {
    authorize_activity_initiator(&db, id.identity, &path.activity_id).await?;

    let mut bytes = web::BytesMut::new();
    while let Some(item) = body.next().await {
        bytes.extend_from_slice(
            &item.map_err(|e| Error::Service(format!("Payload error: {:?}", e)))?,
        );
    }

    let agreement = get_activity_agreement(&db, &path.activity_id).await?;
    let msg = activity::sgx::CallEncryptedService {
        activity_id: path.activity_id.clone(),
        sender: id.identity,
        bytes: bytes.to_vec(),
    };

    let result = ya_net::from(id.identity)
        .to(agreement.provider_id()?.parse()?)
        .service(&activity::exeunit::bus_id(&path.activity_id))
        .send(msg)
        .timeout(query.timeout)
        .await???;

    Ok::<_, Error>(web::Bytes::from(result))
}

#[derive(Deserialize)]
struct PathActivityBatch {
    activity_id: String,
    batch_id: String,
}

fn convert_credentials(
    credentials: ya_core_model::activity::local::Credentials,
) -> Result<Credentials> {
    let cred = match credentials {
        ya_core_model::activity::local::Credentials::Sgx {
            requestor,
            enclave,
            payload_sha3,
            enclave_hash,
            ias_report,
            ias_sig,
            session_key,
        } => Credentials::Sgx(
            SgxCredentials::try_with(
                enclave,
                requestor,
                hex::encode(&payload_sha3),
                hex::encode(&enclave_hash),
                ias_report,
                ias_sig,
                session_key,
            )
            .map_err(|e| Error::Service(format!("Unable to convert SGX credentials: {}", e)))?,
        ),
    };
    Ok(cred)
}
