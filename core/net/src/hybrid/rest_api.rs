use actix_web::{HttpResponse, Responder, Scope};
use ya_client_model::net::{Status, NET_API_V2_NET_PATH};
use ya_service_bus::{typed, RpcEndpoint};

use crate::error::{NetError, Result};

pub fn web_scope() -> Scope {
    actix_web::web::scope(NET_API_V2_NET_PATH).service(get_info)
}

#[actix_web::get("/status")]
async fn get_info() -> Result<impl Responder> {
    let s = typed::service(ya_core_model::net::local::BUS_ID)
        .send(ya_core_model::net::local::Status {})
        .await
        .map_err(|e| NetError::Error(e.into()))?
        .map_err(|e| NetError::Error(e.into()))?;
    let status = Status {
        node_id: s.node_id,
        listen_ip: s.listen_address.map(|addr| addr.to_string()),
        public_ip: s.public_address.map(|addr| addr.to_string()),
        sessions: s.sessions,
    };
    Ok(HttpResponse::Ok().json(status))
}
