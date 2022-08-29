/// Identity service
use futures::lock::Mutex;
use std::sync::Arc;

use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::{Provider, Service};

use crate::cli::Command;

mod appkey;
mod identity;

pub struct Identity;

impl Service for Identity {
    type Cli = Command;
}

impl Identity {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(context: &Context) -> anyhow::Result<()> {
        let db = context.component();

        let service = Arc::new(Mutex::new(
            identity::IdentityService::from_db(db.clone()).await?,
        ));
        identity::IdentityService::bind_service(service);

        identity::wait_for_default_account_unlock(&db).await?;

        appkey::activate(&db).await?;
        Ok(())
    }
}
