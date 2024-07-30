#![allow(dead_code)]

use anyhow::anyhow;

use ya_identity::service::Identity;
use ya_persistence::executor::DbExecutor;

#[derive(Clone)]
pub struct MockIdentity {
    name: String,
    db: DbExecutor,
}

impl MockIdentity {
    pub fn new(name: &str) -> Self {
        let db = Self::create_db(&format!("{name}.identity.db")).unwrap();

        MockIdentity {
            name: name.to_string(),
            db,
        }
    }

    fn create_db(name: &str) -> anyhow::Result<DbExecutor> {
        let db = DbExecutor::in_memory(name)
            .map_err(|e| anyhow!("Failed to create in memory db [{name:?}]. Error: {e}"))?;
        Ok(db)
    }

    pub async fn bind_gsb(&self) -> anyhow::Result<()> {
        log::info!("MockIdentity ({}) - binding GSB", self.name);
        Identity::gsb(&self.db).await?;
        Ok(())
    }
}
