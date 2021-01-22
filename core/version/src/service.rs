use actix_web::{web, HttpResponse, Responder};
use metrics::counter;
use std::time::Duration;
use structopt::StructOpt;

use ya_core_model::version;
use ya_persistence::executor::DbExecutor;
use ya_service_api::{CliCtx, CommandOutput};
use ya_service_api_interfaces::{Provider, Service};
use ya_service_bus::{typed as bus, RpcEndpoint, RpcMessage};
pub type RpcMessageResult<T> = Result<<T as RpcMessage>::Item, <T as RpcMessage>::Error>;

// TODO: uncomment when added
// use crate::{db::migrations};
use crate::db::dao::ReleaseDAO;

/// Yagna version management.
#[derive(StructOpt, Debug)]
pub enum UpgradeCLI {
    /// Stop logging warnings about latest Yagna release availability.
    Skip,

    /// Checks if there is new Yagna version available and shows it.
    Check,
}

impl UpgradeCLI {
    pub async fn run_command(self, _ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            UpgradeCLI::Skip => match bus::service(version::BUS_ID)
                .send(version::Skip {})
                .await??
            {
                Some(r) => CommandOutput::object(format!("skipped: {:?}", r)),
                None => CommandOutput::object("not skipped"),
            },
            UpgradeCLI::Check => CommandOutput::object("not checked"),
            // Ok(Some(release)) => release,
            // Ok(None) => CommandOutput::object(format!(
            //     "Your Yagna is up to date: {}",
            //     ya_compile_time_utils::version_describe!()
            // )),
        }
    }
}

pub struct VersionService;

impl Service for VersionService {
    type Cli = UpgradeCLI;
}

impl VersionService {
    pub async fn gsb<C: Provider<Self, DbExecutor>>(ctx: &C) -> anyhow::Result<()> {
        let db = ctx.component();
        // db.apply_migration(migrations::run_with_output)?;
        bind_gsb(&db);

        // TODO: make interval configurable
        let mut log_interval = tokio::time::interval(Duration::from_secs(30));
        tokio::task::spawn_local(async move {
            loop {
                let release_dao = db.as_dao::<ReleaseDAO>();
                log_interval.tick().await;
                match release_dao.pending_release().await {
                    Ok(Some(release)) => {
                        if !release.seen {
                            log::warn!(
                                "New Yagna version {} ({}) is available. TODO: add howto here",
                                release.name,
                                release.version
                            )
                        }
                    }
                    Ok(None) => log::warn!("no new version yet :-/"),
                    Err(e) => log::error!("while fetching pending release: {}", e),
                }
            }
        });

        Ok(())
    }

    pub fn rest<C: Provider<Self, DbExecutor>>(ctx: &C) -> actix_web::Scope {
        let db: DbExecutor = ctx.component();
        web::scope("").data(db).service(show_version)
    }
}

#[actix_web::get("/version")]
async fn show_version(db: web::Data<DbExecutor>) -> impl Responder {
    match db.as_dao::<ReleaseDAO>().pending_release().await {
        Ok(Some(release)) => HttpResponse::Ok().json(release),
        Ok(None) => HttpResponse::Ok().json(ya_compile_time_utils::semver_str()),
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}

pub fn bind_gsb(db: &DbExecutor) {
    // public for remote requestors interactions
    bus::ServiceBinder::new(version::BUS_ID, db, ())
        .bind(skip_version_gsb)
        .bind(check_version_gsb);

    // Initialize counters to 0 value. Otherwise they won't appear on metrics endpoint
    // until first change to value will be made.
    counter!("version.new", 0);
    counter!("version.skip", 0);
}

async fn skip_version_gsb(
    db: DbExecutor,
    _caller: String,
    _msg: version::Skip,
) -> RpcMessageResult<version::Skip> {
    match db.as_dao::<ReleaseDAO>().skip_pending_release().await {
        Ok(r) => Ok(r.map(|r| r.into())),
        Err(e) => Err(e.to_string().into()),
    }
}

async fn check_version_gsb(
    db: DbExecutor,
    _caller: String,
    _msg: version::Check,
) -> RpcMessageResult<version::Check> {
    crate::notifier::check_release()
        .await
        .map_err(|e| e.to_string())?;

    db.as_dao::<ReleaseDAO>()
        .pending_release()
        .await
        .map(|r| r.map(|r| r.into()))
        .map_err(|e| e.to_string().into())
}
