use metrics::counter;

use ya_core_model::version;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::{typed as bus, RpcMessage};

use crate::db::dao::ReleaseDAO;
use crate::service::cli::ReleaseMessage;

pub type RpcMessageResult<T> = Result<<T as RpcMessage>::Item, <T as RpcMessage>::Error>;

pub fn bind_gsb(db: &DbExecutor) {
    bus::ServiceBinder::new(version::BUS_ID, db, ())
        .bind(skip_version_gsb)
        .bind(get_version_gsb);

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
            counter!("version.skip", 1);
            r
        })),
        Err(e) => Err(e.to_string().into()),
    }
}

async fn get_version_gsb(
    db: DbExecutor,
    _caller: String,
    msg: version::Get,
) -> RpcMessageResult<version::Get> {
    if msg.check {
        crate::github::check_latest_release(&db)
            .await
            .map_err(|e| e.to_string())?;
    }

    db.as_dao::<ReleaseDAO>()
        .version()
        .await
        .map_err(|e| e.to_string().into())
}
