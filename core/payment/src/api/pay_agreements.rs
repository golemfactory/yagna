// External crates
use crate::dao::*;
use crate::utils::*;
use actix_web::web::{get, Data, Path, Query};
use actix_web::{HttpResponse, Scope};
use ya_client_model::payment::params;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .route("/payAgreements", get().to(get_pay_agreements))
        .route("/payAgreements/{agreement_id}", get().to(get_pay_agreement))
        .route(
            "/payAgreements/{agreement_id}/activities",
            get().to(get_pay_agreement_activities),
        )
        .route(
            "/payAgreements/{agreement_id}/orders",
            get().to(get_pay_agreement_orders),
        )
}

async fn get_pay_agreements(
    db: Data<DbExecutor>,
    query: Query<params::FilterParams>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let dao: AgreementDao = db.as_dao();
    let after_timestamp = query.after_timestamp.map(|d| d.naive_utc());
    let max_items = query.max_items;
    match dao
        .get_for_node_id(node_id, after_timestamp, max_items)
        .await
    {
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

async fn get_pay_agreement_orders(
    db: Data<DbExecutor>,
    path: Path<String>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let agreement_id = path.into_inner();
    let dao: BatchDao = db.as_dao();
    match dao
        .get_batch_items(
            node_id,
            BatchItemFilter {
                agreement_id: Some(agreement_id),
                ..Default::default()
            },
        )
        .await
    {
        Ok(items) => response::ok(items),
        Err(e) => response::server_error(&e),
    }
}
