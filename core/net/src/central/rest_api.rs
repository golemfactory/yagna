use actix_web::{HttpResponse, Responder, Scope};
use ya_client_model::net::{Info, NET_API_V2_NET_PATH};

pub fn web_scope() -> Scope {
    actix_web::web::scope(NET_API_V2_NET_PATH).service(get_info)
}

#[actix_web::get("/info")]
async fn get_info() -> impl Responder {
    let info = Info { public_ip: None };
    HttpResponse::Ok().json(info)
}
