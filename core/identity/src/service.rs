use futures::lock::Mutex;

use std::sync::Arc;
/// Identity service
use ya_persistence::executor::DbExecutor;

mod appkey;
mod identity;

pub async fn activate(db: &DbExecutor) -> anyhow::Result<()> {
    log::info!("activating identity service");
    log::debug!("loading default identity");

    let service = Arc::new(Mutex::new(
        identity::IdentityService::from_db(db.clone()).await?,
    ));
    identity::IdentityService::bind_service(service);
    log::info!("identity service activated");

    appkey::activate(db).await?;
    Ok(())
}
