use crate::db::dao::ReleaseDAO;

use ya_client::model::ErrorMessage;
use ya_persistence::executor::DbExecutor;

use actix_web::web::Data;
use actix_web::{web, HttpResponse, Responder};

pub const VERSION_API_PATH: &str = "/version";

pub fn web_scope(db: DbExecutor) -> actix_web::Scope {
    actix_web::web::scope(VERSION_API_PATH)
        .app_data(Data::new(db))
        .service(get_version)
}

#[actix_web::get("/get")]
async fn get_version(db: web::Data<DbExecutor>) -> impl Responder {
    match db.as_dao::<ReleaseDAO>().version().await {
        Ok(v) => HttpResponse::Ok().json(v),
        Err(e) => HttpResponse::InternalServerError().json(ErrorMessage::new(e.to_string())),
    }
}
