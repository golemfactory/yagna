use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::{Provider, Service};

use crate::db::migrations;

pub(crate) mod cli;
mod gsb;
mod rest;

pub struct VersionService;

impl Service for VersionService {
    type Cli = cli::VersionCLI;
}

impl VersionService {
    pub async fn gsb<C: Provider<Self, DbExecutor>>(ctx: &C) -> anyhow::Result<()> {
        let db = ctx.component();
        db.apply_migration(migrations::run_with_output)?;
        crate::notifier::on_start(&db).await?;
        gsb::bind_gsb(&db);

        Ok(())
    }

    pub fn rest<C: Provider<Self, DbExecutor>>(ctx: &C) -> actix_web::Scope {
        rest::web_scope(ctx.component())
    }
}
