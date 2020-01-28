use futures::Future;
use std::pin::Pin;

pub trait Service {
    type Db;
    type Cli;

    fn db<'f>(_: &'f Self::Db) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + 'f>> {
        Box::pin(async move { Ok(()) })
    }

    fn gsb<'f>(_: &'f Self::Db) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + 'f>> {
        Box::pin(async move { Ok(()) })
    }

    fn rest(_: &Self::Db) -> Option<actix_web::Scope> {
        None
    }
}
