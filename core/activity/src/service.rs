use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::{Provider, Service};

use crate::{api, db::migrations, provider};

pub struct Activity;

impl Service for Activity {
    type Cli = crate::cli::ActivityCli;
}

impl Activity {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(ctx: &Context) -> anyhow::Result<()> {
        let db: DbExecutor = ctx.component();
        db.apply_migration(migrations::run_with_output)?;
        provider::service::bind_gsb(&db);
        Ok(())
    }

    pub fn rest<Context: Provider<Self, DbExecutor>>(ctx: &Context) -> actix_web::Scope {
        api::web_scope(&ctx.component())
    }
}
