use ya_client::model::ErrorMessage;
use ya_persistence::executor::DbExecutor;

use actix_web::{web, HttpResponse, Responder};

pub const MISC_API_PATH: &str = "/misc";

pub fn web_scope(db: DbExecutor) -> actix_web::Scope {
    actix_web::web::scope(MISC_API_PATH)
        .data(db)
        .service(get_misc)
}

#[actix_web::get("/get")]
async fn get_misc(db: web::Data<DbExecutor>) -> impl Responder {
    HttpResponse::Ok().json("")
}
