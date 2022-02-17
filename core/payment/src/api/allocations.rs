use std::time::Duration;
// External crates
use actix_web::web::{delete, get, post, put, Data, Json, Path, Query};
use actix_web::{HttpResponse, Scope};
use chrono::Utc;
use serde_json::value::Value::Null;
use ya_client_model::NodeId;

// Workspace uses
use ya_agreement_utils::{ClauseOperator, ConstraintKey, Constraints};
use ya_client_model::payment::*;
use ya_core_model::payment::local::{
    ValidateAllocation, ValidateAllocationError, BUS_ID as LOCAL_SERVICE,
};
use ya_core_model::payment::RpcMessageError;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::{typed as bus, RpcEndpoint};

// Local uses
use crate::dao::*;
use crate::error::{DbError, Error};
use crate::utils::response;
use crate::DEFAULT_PAYMENT_PLATFORM;

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .route("/allocations", post().to(create_allocation))
        .route("/allocations", get().to(get_allocations))
        .route("/allocations/{allocation_id}", get().to(get_allocation))
        .route("/allocations/{allocation_id}", put().to(amend_allocation))
        .route(
            "/allocations/{allocation_id}",
            delete().to(release_allocation),
        )
        .route("/demandDecorations", get().to(get_demand_decorations))
}

async fn create_allocation(
    db: Data<DbExecutor>,
    body: Json<NewAllocation>,
    id: Identity,
) -> HttpResponse {
    // TODO: Handle deposits & timeouts
    let allocation = body.into_inner();
    let node_id = id.identity;
    let payment_platform = allocation
        .payment_platform
        .clone()
        .unwrap_or(DEFAULT_PAYMENT_PLATFORM.to_string());
    let address = allocation.address.clone().unwrap_or(node_id.to_string());

    let validate_msg = ValidateAllocation {
        platform: payment_platform.clone(),
        address: address.clone(),
        amount: allocation.total_amount.clone(),
    };
    match async move { Ok(bus::service(LOCAL_SERVICE).send(validate_msg).await??) }.await {
        Ok(true) => {}
        Ok(false) => return response::bad_request(&"Insufficient funds to make allocation"),
        Err(Error::Rpc(RpcMessageError::ValidateAllocation(
            ValidateAllocationError::AccountNotRegistered,
        ))) => return response::bad_request(&"Account not registered"),
        Err(e) => return response::server_error(&e),
    }

    let db_ = db.clone();
    let dao = db.as_dao::<AllocationDao>();

    match async move {
        let allocation_id = dao
            .create(allocation, node_id, payment_platform, address)
            .await?;
        let allocation_id_ = allocation_id.clone();
        match dao.get(allocation_id, node_id).await? {
            None => Ok(None),
            Some(allocation) => {
                let deadline = allocation.timeout.clone().unwrap_or(Utc::now());
                tokio::task::spawn_local(async move {
                    let deadline = deadline - Utc::now();
                    tokio::time::delay_for(deadline.to_std().unwrap_or(Duration::from_secs(0)))
                        .await;
                    release_alloc(db_, allocation_id_, node_id.clone()).await;
                });
                Ok(Some(allocation))
            }
        }
    }
    .await
    {
        Ok(Some(allocation)) => response::created(allocation),
        Ok(None) => response::server_error(&"Database error"),
        Err(DbError::Query(e)) => response::bad_request(&e),
        Err(e) => response::server_error(&e),
    }
}

async fn get_allocations(
    db: Data<DbExecutor>,
    query: Query<params::FilterParams>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let after_timestamp = query.after_timestamp.map(|d| d.naive_utc());
    let max_items = query.max_items;
    let dao: AllocationDao = db.as_dao();
    match dao.get_for_owner(node_id, after_timestamp, max_items).await {
        Ok(allocations) => response::ok(allocations),
        Err(e) => response::server_error(&e),
    }
}

async fn get_allocation(
    db: Data<DbExecutor>,
    path: Path<params::AllocationId>,
    id: Identity,
) -> HttpResponse {
    let allocation_id = path.allocation_id.clone();
    let node_id = id.identity;
    let dao: AllocationDao = db.as_dao();
    match dao.get(allocation_id, node_id).await {
        Ok(Some(allocation)) => response::ok(allocation),
        Ok(None) => response::not_found(),
        Err(e) => response::server_error(&e),
    }
}

async fn amend_allocation(
    db: Data<DbExecutor>,
    path: Path<params::AllocationId>,
    body: Json<Allocation>,
) -> HttpResponse {
    response::not_implemented() // TODO
}

async fn release_allocation(
    db: Data<DbExecutor>,
    path: Path<params::AllocationId>,
    id: Identity,
) -> HttpResponse {
    let allocation_id = path.allocation_id.clone();
    let node_id = id.identity;

    release_alloc(db, allocation_id, node_id).await
}

async fn get_demand_decorations(
    db: Data<DbExecutor>,
    path: Query<params::AllocationIds>,
    id: Identity,
) -> HttpResponse {
    let allocation_ids = path.allocation_ids.clone();
    let node_id = id.identity;
    let dao: AllocationDao = db.as_dao();
    let allocations = match dao.get_many(allocation_ids, node_id).await {
        Ok(allocations) => allocations,
        Err(e) => return response::server_error(&e),
    };
    if allocations.len() != path.allocation_ids.len() {
        return response::not_found();
    }

    let properties: Vec<MarketProperty> = allocations
        .into_iter()
        .map(|allocation| MarketProperty {
            key: format!(
                "golem.com.payment.platform.{}.address",
                allocation.payment_platform
            ),
            value: allocation.address,
        })
        .collect();
    let constraints = properties
        .iter()
        .map(|property| ConstraintKey::new(property.key.as_str()).equal_to(ConstraintKey::new("*")))
        .collect();
    let constraints = vec![Constraints::new_clause(ClauseOperator::Or, constraints).to_string()];
    response::ok(MarketDecoration {
        properties,
        constraints,
    })
}

pub async fn release_alloc(
    db: Data<DbExecutor>,
    allocation_id: String,
    node_id: NodeId,
) -> HttpResponse {
    let dao = db.as_dao::<AllocationDao>();
    match dao.release(allocation_id, node_id).await {
        Ok(true) => response::ok(Null),
        Ok(false) => response::not_found(),
        Err(e) => response::server_error(&e),
    }
}
