use actix_web::{HttpResponse, Responder, Scope};
use ya_client_model::p2p::{Status, NET_API_V2_NET_PATH};

use crate::error::Result;

pub fn web_scope() -> Scope {
    actix_web::web::scope(NET_API_V2_NET_PATH).service(get_info)
}

#[actix_web::get("/status")]
async fn get_info() -> Result<impl Responder> {
    let (node_id, _) = crate::service::identities().await?;
    let status = Status {
        node_id,
        listen_ip: None,
        public_ip: None,
        sessions: 1,
    };
    Ok(HttpResponse::Ok().json(status))
}
