use actix_web::{web::Data, HttpResponse, Responder, Scope};
use ya_client_model::net::{Info, NET_API_V2_NET_PATH};

use crate::error::Result;

use super::client::ClientProxy;

pub fn web_scope() -> Scope {
    actix_web::web::scope(NET_API_V2_NET_PATH)
        .app_data(Data::new(ClientProxy::new().unwrap()))
        .service(get_info)
}

#[actix_web::get("/info")]
async fn get_info(client: Data<ClientProxy>) -> Result<impl Responder> {
    let public_ip = client.public_addr().await?
        .map(|addr| addr.to_string());
    let info = Info { public_ip };
    Ok(HttpResponse::Ok().json(info))
}
