use actix_web::{web::Data, HttpResponse, Responder, Scope};
use ya_client_model::net::{Status, NET_API_V2_NET_PATH};

use crate::error::Result;

use super::client::ClientProxy;

pub fn web_scope() -> Scope {
    actix_web::web::scope(NET_API_V2_NET_PATH)
        .app_data(Data::new(ClientProxy::new().unwrap()))
        .service(get_info)
}

#[actix_web::get("/status")]
async fn get_info(client: Data<ClientProxy>) -> Result<impl Responder> {
    let status = Status {
        node_id: client.node_id().await?,
        listen_ip: client.bind_addr().await?.map(|addr| addr.to_string()),
        public_ip: client.public_addr().await?.map(|addr| addr.to_string()),
        sessions: client.sessions().await?.len(),
    };
    Ok(HttpResponse::Ok().json(status))
}
