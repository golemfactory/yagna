/// Identity management CLI parser and runner
use anyhow::{Context, Result};
use std::path::PathBuf;
use structopt::*;

use ya_core_model::identity::{self, DEFAULT_IDENTITY};
use ya_service_api::{CliCtx, CommandOutput, ResponseTable};
use ya_service_bus::typed as bus;
use ya_service_bus::RpcEndpoint;
use std::cmp::Reverse;
use ya_core_model::ethaddr::NodeId;
use std::str::FromStr;

const DEFAULT_PASSWORD: &str = "";

#[derive(Debug)]
pub enum NodeOrAlias {
    Node(NodeId),
    Alias(String)
}

impl std::str::FromStr for NodeOrAlias {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(if s.starts_with("0x") {
            match NodeId::from_str(s) {
                Ok(node_id) => NodeOrAlias::Node(node_id),
                Err(e) => NodeOrAlias::Alias(s.to_owned())
            }
        }
        else {
            NodeOrAlias::Alias(s.to_owned())
        })
    }
}

impl NodeOrAlias {

    async fn resolve(&self) -> anyhow::Result<NodeId> {
        match self {
            NodeOrAlias::Node(node_id) => Ok(node_id.clone()),
            NodeOrAlias::Alias(alias) => {
                let id = bus::service(identity::BUS_ID)
                    .send(identity::Get::ByAlias(alias.to_owned()))
                    .await.map_err(|e| anyhow::anyhow!(e))?;
                match id? {
                    Some(id) => Ok(id.node_id),
                    None => anyhow::bail!("node with alias {} not found", alias)
                }
            }
        }
    }

}

#[derive(StructOpt, Debug)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
pub enum IdentityCommand {
    /// Show list of all identities
    List {
    },

    /// Display identity
    Show {
        /// Identity alias to show
        alias: String,
    },

    Lock {
        /// NodeId or key
        alias : String,
    },

    /// Create identity
    Create {
        /// Identity alias to create
        alias: Option<String>,

        /// Existing keystore to use
        #[structopt(long = "from-keystore")]
        from_keystore: Option<PathBuf>,

        /// password for keystore
        #[structopt(short, long)]
        #[structopt(default_value = DEFAULT_PASSWORD)]
        password: String,

        /// force recreation of existing identity
        #[structopt(short, long)]
        force: bool,
    },
    /// Update given identity
    Update {
        /// Identity to update
        #[structopt(default_value = DEFAULT_IDENTITY)]
        alias_or_id: NodeOrAlias,
        #[structopt(long)]
        alias : Option<String>,
        #[structopt(long = "set-default")]
        set_default : bool,
    },

    /// Drop given identity
    Drop {
        /// Identity alias to drop
        alias: String,
    },
}

impl IdentityCommand {
    pub async fn run_command(&self, ctx: &CliCtx) -> Result<CommandOutput> {
        match self {
            IdentityCommand::List { .. } => {
                let mut identities: Vec<identity::IdentityInfo> = bus::service(identity::BUS_ID)
                    .send(identity::List::default())
                    .await
                    .map_err(|e| anyhow::Error::msg(e))
                    .context("sending id List to BUS")?
                    .unwrap();
                identities.sort_by_key(|id| Reverse((id.is_default, id.alias.clone())));
                Ok(ResponseTable {
                    columns: vec!["default".into(), "alias".into(), "address".into()],
                    values: identities
                        .into_iter()
                        .map(|identity| {
                            serde_json::json! {[
                                if identity.is_default { "X" } else { " " },
                                identity.alias,
                                identity.node_id
                            ]}
                        })
                        .collect(),
                }
                .into())
            }
            IdentityCommand::Show { alias} => CommandOutput::object(
                bus::service(identity::BUS_ID)
                    .send(identity::Get::ByAlias(alias.into()))
                    .await
                    .map_err(|e| anyhow::Error::msg(e))?,
            ),
            IdentityCommand::Update {
                alias_or_id,
                alias,
                set_default
            } => {
                let node_id = alias_or_id.resolve().await?;
                let id = bus::service(identity::BUS_ID).send(
                    identity::Update::with_id(node_id)
                        .with_alias(alias.clone())
                        .with_default(*set_default)
                ).await
                    .map_err(|e| anyhow::Error::msg(e))?;
                CommandOutput::object(id)
            },
            IdentityCommand::Create {
                alias,
                from_keystore,
                password,
                force,
            } => {
                let id = bus::service(identity::BUS_ID)
                    .send(identity::CreateGenerated {
                        alias: alias.clone(),
                    })
                    .await
                    .map_err(|e| anyhow::Error::msg(e))?;
                CommandOutput::object(id)
            }
            _ => {
                // TODO:
                unimplemented!()
            }
        }
    }
}
