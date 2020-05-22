use crate::{api, provider};

use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::{Provider, Service};

pub struct Activity;

impl Service for Activity {
    type Cli = ();
}

impl Activity {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(ctx: &Context) -> anyhow::Result<()> {
        provider::service::bind_gsb(&ctx.component());
        Ok(())
    }

    pub fn rest<Context: Provider<Self, DbExecutor>>(ctx: &Context) -> actix_web::Scope {
        api::web_scope(&ctx.component())
    }
}
