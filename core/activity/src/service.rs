use crate::{api, provider};

use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::{Provider, Service};

pub struct Activity;

impl Service for Activity {
    type Cli = ();
}

impl Activity {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(ctx: &Context) -> anyhow::Result<()> {
        let db = ctx.component();
        provider::service::bind_gsb(&db);
        Ok(())
    }

    pub fn rest(db: &DbExecutor) -> actix_web::Scope {
        api::web_scope(&db)
    }
}
