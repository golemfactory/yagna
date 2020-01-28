use crate::{api, provider};
use futures::Future;
use std::pin::Pin;
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::Service;

pub struct Activity;

impl Service for Activity {
    type Db = DbExecutor;
    type Cli = ();

    fn gsb<'f>(db: &'f DbExecutor) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + 'f>> {
        Box::pin(async move {
            provider::service::bind_gsb(&db);
            Ok(())
        })
    }

    fn rest(db: &DbExecutor) -> Option<actix_web::Scope> {
        Some(api::web_scope(&db))
    }
}
