/// Identity service
use futures::lock::Mutex;
use std::sync::Arc;

use crate::cli::Command;

use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::{Provider, Service};

mod appkey;
mod identity;

pub struct Identity;

impl Service for Identity {
    type Cli = Command;
}

impl Identity {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(context: &Context) -> anyhow::Result<()> {
        log::info!("activating identity service");
        log::debug!("loading default identity");
        let db = context.component();

        let service = Arc::new(Mutex::new(
            identity::IdentityService::from_db(db.clone()).await?,
        ));
        identity::IdentityService::bind_service(service);
        log::info!("identity service activated");

        appkey::activate(&db).await?;
        Ok(())
    }
}
