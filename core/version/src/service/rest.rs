use crate::db::dao::ReleaseDAO;
use crate::db::model::DBRelease;

use ya_client::model::ErrorMessage;
use ya_persistence::executor::DbExecutor;

use actix_web::{web, HttpResponse, ResponseError};
use anyhow::anyhow;
use serde::{Deserialize, Serialize};

pub const VERSION_API_PATH: &str = "";

#[derive(Serialize, Deserialize)]
struct VersionInfo {
    pub current: DBRelease,
    pub pending: Option<DBRelease>,
}

pub fn web_scope(db: DbExecutor) -> actix_web::Scope {
    actix_web::web::scope(VERSION_API_PATH)
        .data(db)
        .service(get_version)
}

#[actix_web::get("/version")]
async fn get_version(db: web::Data<DbExecutor>) -> Result<HttpResponse, VersionError> {
    Ok(HttpResponse::Ok().json(VersionInfo {
        current: db
            .as_dao::<ReleaseDAO>()
            .current_release()
            .await
            .map_err(VersionError::from)?
            .ok_or(anyhow!("Can't determine current version."))
            .map_err(VersionError::from)?,
        pending: db
            .as_dao::<ReleaseDAO>()
            .pending_release()
            .await
            .map_err(VersionError::from)?,
    }))
}

#[derive(thiserror::Error, Debug)]
#[error("Error querying version. {0}.")]
pub struct VersionError(#[from] anyhow::Error);

impl ResponseError for VersionError {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::InternalServerError().json(ErrorMessage::new(self.to_string()))
    }
}
