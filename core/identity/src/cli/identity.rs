/// Identity management CLI parser and runner
use std::path::PathBuf;
use structopt::*;

use ethsign::Protected;
use std::cmp::Reverse;
use ya_client_model::NodeId;
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

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
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
            NodeOrAlias::Alias(alias) => Ok(bus::service(identity::BUS_ID)
                .send(identity::Get::ByAlias(alias.to_owned()))
                .await??
                .node_id),
            NodeOrAlias::DefaultNode => Ok(bus::service(identity::BUS_ID)
                .send(identity::Get::ByDefault)
                .await??
                .node_id),
        }
    }
}

impl From<NodeOrAlias> for identity::Get {
    fn from(noa: NodeOrAlias) -> Self {
        match noa {
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

    /// Exports given identity to a file | stdout
    Export {
        /// Identity alias to export
        node_or_alias: Option<NodeOrAlias>,

        /// File path where identity will be written. Defaults to `stdout`
        #[structopt(long = "file-path")]
        file_path: Option<PathBuf>,
    },
}

impl IdentityCommand {
    pub async fn run_command(&self, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            IdentityCommand::List { .. } => {
                let mut identities = bus::service(identity::BUS_ID)
                    .send(identity::List::default())
                    .await??;
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
                let id = bus::service(identity::BUS_ID).send(command).await??;
                // We return Ok, just to be backward compatible, but it is ugly
                CommandOutput::object(Ok::<_, ()>(id))
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
                    .await??;
                // We return Ok, just to be backward compatible, but it is ugly
                CommandOutput::object(Ok::<_, ()>(id))
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
                    .await??;
                // We return Ok, just to be backward compatible, but it is ugly
                CommandOutput::object(Ok::<_, ()>(id))
            }
            IdentityCommand::Lock { node_or_alias } => {
                let node_id = node_or_alias.clone().unwrap_or_default().resolve().await?;
                let id = bus::service(identity::BUS_ID)
                    .send(identity::Lock::with_id(node_id))
                    .await??;
                // We return Ok, just to be backward compatible, but it is ugly
                CommandOutput::object(Ok::<_, ()>(id))
            }
            IdentityCommand::Unlock { node_or_alias } => {
                let node_id = node_or_alias.clone().unwrap_or_default().resolve().await?;
                let password = rpassword::read_password_from_tty(Some("Password: "))?;
                let id = bus::service(identity::BUS_ID)
                    .send(identity::Unlock::with_id(node_id, password))
                    .await??;
                // We return Ok, just to be backward compatible, but it is ugly
                CommandOutput::object(Ok::<_, ()>(id))
            }
            IdentityCommand::Drop { node_or_alias } => {
                let node_id = node_or_alias.resolve().await?;

                let id = bus::service(identity::BUS_ID)
                    .send(identity::Drop::with_id(node_id))
                    .await??;
                // We return Ok, just to be backward compatible, but it is ugly
                CommandOutput::object(Ok::<_, ()>(id))
            }
            IdentityCommand::Export {
                node_or_alias,
                file_path,
            } => {
                let node_id = node_or_alias.clone().unwrap_or_default().resolve().await?;
                let key_file = bus::service(identity::BUS_ID)
                    .send(identity::GetKeyFile(node_id))
                    .await??;

                match file_path {
                    Some(file) => {
                        if file.is_file() {
                            anyhow::bail!("File already exists")
                        }

                        std::fs::write(file, key_file)?;
                        CommandOutput::object(format!("Written to '{}'", file.display()))
                    }
                    None => {
                        if ctx.json_output {
                            Ok(CommandOutput::Object(serde_json::from_str(&key_file)?))
                        } else {
                            CommandOutput::object(key_file)
                        }
                    }
                }
            }
        }
    }
}
