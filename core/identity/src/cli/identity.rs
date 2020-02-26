/// Identity management CLI parser and runner
use anyhow::{Context, Result};
use std::path::PathBuf;
use structopt::*;

use ethsign::Protected;
use std::cmp::Reverse;
use ya_core_model::ethaddr::NodeId;
use ya_core_model::identity::{self};
use ya_service_api::{CliCtx, CommandOutput, ResponseTable};
use ya_service_bus::typed as bus;
use ya_service_bus::RpcEndpoint;

#[derive(Debug, Clone)]
pub enum NodeOrAlias {
    Node(NodeId),
    Alias(String),
    DefaultNode,
}

impl Default for NodeOrAlias {
    fn default() -> Self {
        NodeOrAlias::DefaultNode
    }
}

impl std::str::FromStr for NodeOrAlias {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Ok(NodeOrAlias::DefaultNode);
        }
        Ok(if s.starts_with("0x") {
            match NodeId::from_str(s) {
                Ok(node_id) => NodeOrAlias::Node(node_id),
                Err(_e) => NodeOrAlias::Alias(s.to_owned()),
            }
        } else {
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
                    .await
                    .map_err(|e| anyhow::anyhow!(e))?;
                match id? {
                    Some(id) => Ok(id.node_id),
                    None => anyhow::bail!("node with alias {} not found", alias),
                }
            }
            NodeOrAlias::DefaultNode => {
                let id = bus::service(identity::BUS_ID)
                    .send(identity::Get::ByDefault)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))?;
                match id? {
                    Some(id) => Ok(id.node_id),
                    None => anyhow::bail!("default node not found"),
                }
            }
        }
    }
}

impl Into<identity::Get> for NodeOrAlias {
    fn into(self) -> identity::Get {
        match self {
            NodeOrAlias::DefaultNode => identity::Get::ByDefault,
            NodeOrAlias::Alias(alias) => identity::Get::ByAlias(alias),
            NodeOrAlias::Node(node_id) => identity::Get::ByNodeId(node_id),
        }
    }
}

#[derive(StructOpt, Debug)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
/// Identity management
pub enum IdentityCommand {
    /// Show list of all identities
    List {},

    /// Display identity
    Show {
        /// Identity alias to show
        node_or_alias: Option<NodeOrAlias>,
    },

    /// Locks identity
    Lock {
        /// NodeId or key
        node_or_alias: Option<NodeOrAlias>,
    },

    Unlock {
        node_or_alias: Option<NodeOrAlias>,
    },

    /// Create identity
    Create {
        /// Identity alias to create
        alias: Option<String>,

        /// Existing keystore to use
        #[structopt(long = "from-keystore")]
        from_keystore: Option<PathBuf>,

        /// password for keystore
        #[structopt(long = "no-password")]
        no_password: bool,
    },
    /// Update given identity
    Update {
        /// Identity to update
        #[structopt(default_value = "")]
        alias_or_id: NodeOrAlias,
        #[structopt(long)]
        alias: Option<String>,
        #[structopt(long = "set-default")]
        set_default: bool,
    },

    /// Drop given identity
    Drop {
        /// Identity alias to drop
        node_or_alias: NodeOrAlias,
    },
}

impl IdentityCommand {
    pub async fn run_command(&self, _ctx: &CliCtx) -> Result<CommandOutput> {
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
                    columns: vec![
                        "default".into(),
                        "locked".into(),
                        "alias".into(),
                        "address".into(),
                    ],
                    values: identities
                        .into_iter()
                        .map(|identity| {
                            serde_json::json! {[
                                if identity.is_default { "X" } else { "" },
                                if identity.is_locked { "X" } else { "" },
                                identity.alias,
                                identity.node_id
                            ]}
                        })
                        .collect(),
                }
                .into())
            }
            IdentityCommand::Show { node_or_alias } => {
                let command: identity::Get = node_or_alias.clone().unwrap_or_default().into();
                CommandOutput::object(
                    bus::service(identity::BUS_ID)
                        .send(command)
                        .await
                        .map_err(|e| anyhow::Error::msg(e))?,
                )
            }
            IdentityCommand::Update {
                alias_or_id,
                alias,
                set_default,
            } => {
                let node_id = alias_or_id.resolve().await?;
                let id = bus::service(identity::BUS_ID)
                    .send(
                        identity::Update::with_id(node_id)
                            .with_alias(alias.clone())
                            .with_default(*set_default),
                    )
                    .await
                    .map_err(|e| anyhow::Error::msg(e))?;
                CommandOutput::object(id)
            }
            IdentityCommand::Create {
                alias,
                from_keystore,
                no_password,
            } => {
                let key_file = if let Some(keystore) = from_keystore {
                    std::fs::read_to_string(keystore)?
                } else {
                    let password = if *no_password {
                        Protected::from("")
                    } else {
                        let password: Protected =
                            rpassword::read_password_from_tty(Some("Password: "))?.into();
                        let password2: Protected =
                            rpassword::read_password_from_tty(Some("Confirm password: "))?.into();
                        if password.as_ref() != password2.as_ref() {
                            anyhow::bail!("Password and confirmation do not match.")
                        }
                        password
                    };
                    crate::id_key::generate_new_keyfile(password)?
                };

                let id = bus::service(identity::BUS_ID)
                    .send(identity::CreateGenerated {
                        alias: alias.clone(),
                        from_keystore: Some(key_file),
                    })
                    .await
                    .map_err(|e| anyhow::Error::msg(e))?;
                CommandOutput::object(id)
            }
            IdentityCommand::Lock { node_or_alias } => {
                let node_id = node_or_alias.clone().unwrap_or_default().resolve().await?;
                CommandOutput::object(
                    bus::service(identity::BUS_ID)
                        .send(identity::Lock::with_id(node_id))
                        .await
                        .map_err(|e| anyhow::Error::msg(e))?,
                )
            }
            IdentityCommand::Unlock { node_or_alias } => {
                let node_id = node_or_alias.clone().unwrap_or_default().resolve().await?;
                let password = rpassword::read_password_from_tty(Some("Password: "))?;
                CommandOutput::object(
                    bus::service(identity::BUS_ID)
                        .send(identity::Unlock::with_id(node_id, password))
                        .await
                        .map_err(|e| anyhow::Error::msg(e))?,
                )
            }
            IdentityCommand::Drop { node_or_alias } => {
                let command: identity::Get = node_or_alias.clone().into();
                let id = bus::service(identity::BUS_ID)
                    .send(command)
                    .await
                    .map_err(|e| anyhow::Error::msg(e))?;
                let id = match id {
                    Ok(Some(v)) => v,
                    Err(e) => return CommandOutput::object(Err::<(), _>(e)),
                    Ok(None) => anyhow::bail!("identity not found"),
                };

                if id.is_default {
                    anyhow::bail!("default identity")
                }
                CommandOutput::object(
                    bus::service(identity::BUS_ID)
                        .send(identity::DropId::with_id(id.node_id))
                        .await
                        .map_err(|e| anyhow::Error::msg(e))?,
                )
            }
        }
    }
}
