/// Identity service
use futures::lock::Mutex;
use std::sync::Arc;

use ya_core_model as model;
use ya_core_model::bus::GsbBindPoints;
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
        Self::gsb_prefixed(context, None).await
    }

    pub async fn gsb_prefixed<Context: Provider<Self, DbExecutor>>(
        context: &Context,
        base: Option<GsbBindPoints>,
    ) -> anyhow::Result<()> {
        let db = context.component();
        let gsb = base.unwrap_or_default();
        let gsb_ident = Arc::new(gsb.clone().service(model::identity::BUS_SERVICE_NAME));

        let service = Arc::new(Mutex::new(
            identity::IdentityService::from_db(db.clone()).await?,
        ));
        identity::IdentityService::bind_service(service, gsb_ident.clone());

        identity::wait_for_default_account_unlock(gsb_ident.clone()).await?;

        let gsb_appkey = Arc::new(gsb.service(model::appkey::BUS_SERVICE_NAME));
        appkey::activate(&db, gsb_appkey.clone()).await?;
        Ok(())
    }
}
