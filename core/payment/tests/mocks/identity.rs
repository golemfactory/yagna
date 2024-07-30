#![allow(dead_code)]

use anyhow::{anyhow, bail};
use serde::de::DeserializeOwned;
use ya_client_model::NodeId;
use ya_core_model::appkey::AppKey;
use ya_core_model::identity::IdentityInfo;
use ya_identity::cli::{AppKeyCommand, IdentityCommand};

use ya_identity::service::Identity;
use ya_persistence::executor::DbExecutor;
use ya_service_api::{CliCtx, CommandOutput};

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

    pub async fn create_identity_key(&self, name: &str) -> anyhow::Result<AppKey> {
        let identity: IdentityInfo = self
            .create_identity(name)
            .await
            .map_err(|e| anyhow!("Creating Identity: {e}"))?;
        let appkey = self
            .create_appkey(name, identity.node_id)
            .await
            .map_err(|e| anyhow!("Creating AppKey: {e}"))?;

        Ok(appkey)
    }

    pub async fn create_identity(&self, name: &str) -> anyhow::Result<IdentityInfo> {
        let ctx = CliCtx::default();
        let command = IdentityCommand::Create {
            no_password: true,
            alias: Some(name.to_string()),
            password: None,
            from_keystore: None,
            from_private_key: None,
        };

        Ok(parse_output_result::<IdentityInfo>(
            command.run_command(&ctx).await?,
        )?)
    }
    pub async fn create_appkey(&self, name: &str, id: NodeId) -> anyhow::Result<AppKey> {
        let ctx = CliCtx::default();
        let command = AppKeyCommand::Create {
            name: name.to_string(),
            role: "manager".to_string(),
            id: Some(id.to_string()),
            allow_origins: vec![],
        };
        let _key = command.run_command(&ctx).await?;

        let output = AppKeyCommand::Show {
            name: name.to_string(),
        }
        .run_command(&ctx)
        .await?;

        Ok(parse_output::<AppKey>(output)?)
    }
}

fn parse_output_result<T: DeserializeOwned>(output: CommandOutput) -> anyhow::Result<T> {
    Ok(match output {
        CommandOutput::Object(json) => serde_json::from_value::<Result<T, String>>(json)
            .map_err(|e| anyhow!("Error parsing command response: {e}"))?
            .map_err(|e| anyhow!("Command failed: {e}"))?,
        _ => bail!("Unexpected output: {output:?}"),
    })
}

fn parse_output<T: DeserializeOwned>(output: CommandOutput) -> anyhow::Result<T> {
    Ok(match output {
        CommandOutput::Object(json) => serde_json::from_value::<T>(json)
            .map_err(|e| anyhow!("Error parsing command response: {e}"))?,
        _ => bail!("Unexpected output: {output:?}"),
    })
}
