use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::{Provider, Service};

pub(crate) mod cli;
mod gsb;
mod rest;

pub struct MiscService;

impl Service for MiscService {
    type Cli = cli::MiscCLI;
}

impl MiscService {
    pub async fn gsb<C: Provider<Self, DbExecutor>>(ctx: &C) -> anyhow::Result<()> {
        crate::notifier::on_start(&ctx.component()).await?;
        gsb::bind_gsb(&ctx.component());

        Ok(())
    }

    pub fn rest<C: Provider<Self, DbExecutor>>(ctx: &C) -> actix_web::Scope {
        rest::web_scope(ctx.component())
    }
}
