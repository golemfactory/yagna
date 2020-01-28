/// Identity service
use futures::lock::Mutex;
use std::sync::Arc;

use crate::cli::Command;
use futures::Future;
use std::pin::Pin;
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::Service;

mod appkey;
mod identity;

pub struct Identity;

impl Service for Identity {
    type Db = DbExecutor;
    type Cli = Command;

    fn gsb<'f>(db: &'f DbExecutor) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + 'f>> {
        Box::pin(async move {
            log::info!("activating identity service");
            log::debug!("loading default identity");

            let service = Arc::new(Mutex::new(
                identity::IdentityService::from_db(db.clone()).await?,
            ));
            identity::IdentityService::bind_service(service);
            log::info!("identity service activated");

            appkey::activate(db).await?;
            Ok(())
        })
    }
}
