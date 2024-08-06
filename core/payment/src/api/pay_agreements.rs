// External crates
use crate::dao::*;
use crate::utils::*;
use actix_web::web::{get, Data, Path};
use actix_web::{HttpResponse, Scope};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .route("/payAgreements", get().to(get_pay_agreements))
        .route("/payAgreement/{agreement_id}", get().to(get_pay_agreement))
        .route(
            "/payAgreement/{agreement_id}/activities",
            get().to(get_pay_agreement_activities),
        )
}

async fn get_pay_agreements(db: Data<DbExecutor>, id: Identity) -> HttpResponse {
    let node_id = id.identity;
    let dao: AgreementDao = db.as_dao();
    match dao.list(None).await {
        Ok(agreements) => response::ok(agreements),
        Err(e) => response::server_error(&e),
    }
}

async fn get_pay_agreement(db: Data<DbExecutor>, path: Path<String>, id: Identity) -> HttpResponse {
    let node_id = id.identity;
    let agreement_id = path.into_inner();
    let dao: AgreementDao = db.as_dao();
    match dao.get(agreement_id, node_id).await {
        Ok(agreement) => response::ok(agreement),
        Err(e) => response::server_error(&e),
    }
}

async fn get_pay_agreement_activities(
    db: Data<DbExecutor>,
    path: Path<String>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let agreement_id = path.into_inner();
    let dao: ActivityDao = db.as_dao();
    match dao.list(None, Some(agreement_id)).await {
        Ok(activities) => response::ok(activities),
        Err(e) => response::server_error(&e),
    }
}
