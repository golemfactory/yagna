/// Identity management CLI parser and runner
use anyhow::{Context, Result};
use std::{fs, path::PathBuf};
use structopt::*;

use ya_core_model::identity::{self, DEFAULT_IDENTITY};
use ya_service_api::{CliCtx, CommandOutput, ResponseTable};
use ya_service_bus::typed as bus;
use ya_service_bus::RpcEndpoint;

const KEYS_SUBDIR: &str = "keys";
const DEFAULT_PASSWORD: &str = "";

#[derive(StructOpt, Debug)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
pub enum IdentityCommand {
    /// Show list of all identities
    List {
        /// password for keystore
        #[structopt(short, long)]
        #[structopt(default_value = DEFAULT_PASSWORD)]
        password: String,
    },

    /// Display identity
    Show {
        /// Identity alias to show
        #[structopt(default_value = DEFAULT_IDENTITY)]
        alias: String,

        /// password for keystore
        #[structopt(short, long)]
        #[structopt(default_value = DEFAULT_PASSWORD)]
        password: String,
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
        /// Identity alias to update
        #[structopt(default_value = DEFAULT_IDENTITY)]
        alias: String,

        /// password for keystore
        #[structopt(short, long)]
        #[structopt(default_value = DEFAULT_PASSWORD)]
        password: String,

        /// password for keystore
        #[structopt(short, long)]
        new_password: String,
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
                use ya_service_bus::RpcEndpoint;
                let identities: Vec<identity::IdentityInfo> = bus::service(identity::BUS_ID)
                    .send(identity::List::default())
                    .await
                    .map_err(|e| anyhow::Error::msg(e))
                    .context("sending id List to BUS")?
                    .unwrap();
                Ok(ResponseTable {
                    columns: vec!["alias".into(), "address".into()],
                    values: identities
                        .into_iter()
                        .map(|identity| {
                            serde_json::json! {
                                [identity.alias, identity.node_id]
                            }
                        })
                        .collect(),
                }
                    .into())
            }
            IdentityCommand::Show { alias, password } => CommandOutput::object(
                bus::service(identity::BUS_ID)
                    .send(identity::Get::ByAlias(alias.into()))
                    .await
                    .map_err(|e| anyhow::Error::msg(e))?,
            ),
            IdentityCommand::Update {
                alias,
                password,
                new_password,
            } => unimplemented!(),
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
            IdentityCommand::Drop { alias } => {
                // TODO:
                unimplemented!()
            }
        }
    }
}
