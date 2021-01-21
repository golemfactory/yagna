use crate::db::model::Release;
use crate::notifier::check_release;

use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::{Provider, Service};
use ya_service_api_web::middleware::Identity;

use actix_web::{web, HttpResponse, Responder};
use chrono::DateTime;
use serde::{Deserialize, Serialize};

pub const VERSION_API_PATH: &str = "/version-api/v1/";

pub struct VersionService;

impl Service for VersionService {
    type Cli = ();
}

impl VersionService {
    pub fn rest<Context: Provider<Self, DbExecutor>>(ctx: &Context) -> actix_web::Scope {
        actix_web::web::scope(VERSION_API_PATH)
            .data(ctx.component())
            .service(get_version)
    }
}

#[derive(Serialize, Deserialize)]
struct VersionInfo {
    pub current: Release,
    pub pending: Option<Release>,
}

#[actix_web::get("/version")]
async fn get_version(db: web::Data<DbExecutor>, _id: Identity) -> impl Responder {
    // TODO: Should we validate identity??
    let last_release = check_release().await.unwrap().into_iter().last().unwrap();

    let last_release = Release {
        version: last_release.version,
        name: last_release.name,
        seen: false,
        release_ts: DateTime::parse_from_rfc3339(&last_release.date)
            .unwrap()
            .naive_utc(),
        insertion_ts: None,
        update_ts: None,
    };

    HttpResponse::Ok().json(VersionInfo {
        current: last_release.clone(),
        pending: Some(last_release),
    })
}
