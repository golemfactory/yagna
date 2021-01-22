use metrics::counter;
use structopt::StructOpt;

use ya_core_model::version;
use ya_persistence::executor::DbExecutor;
use ya_service_api::{CliCtx, CommandOutput};
use ya_service_api_interfaces::{Provider, Service};
use ya_service_bus::{typed as bus, RpcEndpoint, RpcMessage};
pub type RpcMessageResult<T> = Result<<T as RpcMessage>::Item, <T as RpcMessage>::Error>;

use crate::db::{dao::ReleaseDAO, migrations};
use crate::notifier::ReleaseMessage;

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
                Some(r) => {
                    counter!("version.skip", 1);
                    CommandOutput::object(ReleaseMessage::Skipped(&r).to_string())
                }
                None => CommandOutput::object("No pending release to skip."),
            },
            UpgradeCLI::Check => match bus::service(version::BUS_ID)
                .send(version::Check {})
                .await??
            {
                Some(r) => CommandOutput::object(ReleaseMessage::Available(&r).to_string()),
                None => CommandOutput::object(format!(
                    "Your Yagna is up to date -- {}",
                    ya_compile_time_utils::version_describe!()
                )),
            },
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
        db.apply_migration(migrations::run_with_output)?;
        crate::notifier::on_start(&db).await?;
        bind_gsb(&db);

        Ok(())
    }

    pub fn rest<C: Provider<Self, DbExecutor>>(ctx: &C) -> actix_web::Scope {
        crate::rest::web_scope(ctx.component())
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
        Ok(r) => Ok(r.map(|r| {
            let r = r.into();
            log::info!("{}", ReleaseMessage::Skipped(&r));
            r
        })),
        Err(e) => Err(e.to_string().into()),
    }
}

async fn check_version_gsb(
    db: DbExecutor,
    _caller: String,
    _msg: version::Check,
) -> RpcMessageResult<version::Check> {
    crate::notifier::check_latest_release(&db)
        .await
        .map_err(|e| e.to_string())?;

    db.as_dao::<ReleaseDAO>()
        .pending_release()
        .await
        .map(|r| r.map(|r| r.into()))
        .map_err(|e| e.to_string().into())
}
