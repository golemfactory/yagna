use crate::dao::*;
use crate::utils::*;
use actix_web::web::{get, Data, Path, Query};
use actix_web::{HttpResponse, Scope};
use ya_client_model::payment::params;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
//
pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .route("/batchOrders", get().to(get_batch_orders))
        .route("/batchOrders/{order_id}", get().to(get_batch_order))
        .route(
            "/batchOrders/{order_id}/items",
            get().to(get_batch_order_items),
        )
        .route(
            "/batchOrders/{order_id}/items/{payee_addr}/details",
            get().to(get_batch_order_item_details),
        )
}

async fn get_batch_orders(
    db: Data<DbExecutor>,
    query: Query<params::FilterParams>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let dao: BatchDao = db.as_dao();
    let after_timestamp = query.after_timestamp.map(|d| d.naive_utc());
    let max_items = query.max_items;
    match dao
        .get_for_node_id(node_id, after_timestamp, max_items)
        .await
    {
        Ok(batch_orders) => response::ok(batch_orders),
        Err(e) => response::server_error(&e),
    }
}

async fn get_batch_order(db: Data<DbExecutor>, path: Path<String>, id: Identity) -> HttpResponse {
    let node_id = id.identity;
    let batch_order_id = path.into_inner();
    let dao: BatchDao = db.as_dao();
    match dao.get_batch_order(batch_order_id, node_id).await {
        Ok(batch_order) => response::ok(batch_order),
        Err(e) => response::server_error(&e),
    }
}

async fn get_batch_order_items(
    db: Data<DbExecutor>,
    path: Path<String>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let batch_order_id = path.into_inner();
    let dao: BatchDao = db.as_dao();
    match dao.get_batch_order_items(batch_order_id, node_id).await {
        Ok(batch_order_items) => response::ok(batch_order_items),
        Err(e) => response::server_error(&e),
    }
}

async fn get_batch_order_item_details(
    db: Data<DbExecutor>,
    path: Path<(String, String)>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let (batch_order_id, payee_addr) = path.into_inner();
    let dao: BatchDao = db.as_dao();
    match dao
        .get_batch_items(
            node_id,
            BatchItemFilter {
                order_id: Some(batch_order_id),
                payee_addr: Some(payee_addr),
                ..Default::default()
            },
        )
        .await
    {
        Ok(items) => response::ok(items),
        Err(e) => response::server_error(&e),
    }
}
