use actix_web::{HttpResponse, Scope};
use ya_client_model::net::NET_API_V2_NET_PATH;

use crate::error::{NetError, Result};

pub fn web_scope() -> Scope {
    actix_web::web::scope(NET_API_V2_NET_PATH).service(get_info)
}

#[actix_web::get("/status")]
async fn get_info() -> Result<HttpResponse> {
    Err(NetError::BadRequest(
        "Not implemented for Central network".to_string(),
    ))
}
