use anyhow::{Error, Result};
use structopt::*;

use ya_core_model::appkey as model;
use ya_service_api::{CliCtx, Command, CommandOutput, ResponseTable};
use ya_service_bus::{typed as bus, RpcEndpoint};

#[derive(StructOpt, Debug)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
pub enum AppKeyCommand {
    Create {
        name: String,
        #[structopt(default_value = model::DEFAULT_ROLE, short, long)]
        role: String,
        #[structopt(default_value = model::DEFAULT_IDENTITY, long)]
        id: String,
    },
    Drop {
        name: String,
        #[structopt(long)]
        id: Option<String>,
    },
    List {
        #[structopt(long)]
        id: Option<String>,
        #[structopt(default_value = "1", short, long)]
        page: u32,
        #[structopt(default_value = "10", long)]
        per_page: u32,
    },
}

impl Command for AppKeyCommand {
    fn run_command(&self, ctx: &CliCtx) -> Result<CommandOutput, Error> {
        match &self {
            AppKeyCommand::Create { name, role, id } => {
                let fut = async {
                    let create = model::Create {
                        name: name.clone(),
                        role: role.clone(),
                        identity: id.clone(),
                    };
                    let _ = bus::service(model::ID)
                        .send(create)
                        .await
                        .map_err(|e| anyhow::Error::msg(e))?
                        .unwrap();
                    Ok(CommandOutput::NoOutput)
                };

                futures::pin_mut!(fut);
                ctx.block_on(fut)
            }
            AppKeyCommand::Drop { name, id } => {
                let fut = async {
                    let remove = model::Remove {
                        name: name.clone(),
                        identity: id.clone(),
                    };
                    let _ = bus::service(model::ID)
                        .send(remove)
                        .await
                        .map_err(|e| anyhow::Error::msg(e))?
                        .unwrap();
                    Ok(CommandOutput::NoOutput)
                };

                futures::pin_mut!(fut);
                ctx.block_on(fut)
            }
            AppKeyCommand::List { id, page, per_page } => {
                let fut = async {
                    let list = model::List {
                        identity: id.clone(),
                        page: page.clone(),
                        per_page: per_page.clone(),
                    };
                    let result: (Vec<model::AppKey>, u32) = bus::service(model::ID)
                        .send(list)
                        .await
                        .map_err(|e| anyhow::Error::msg(e))?
                        .unwrap();

                    Ok(ResponseTable {
                        columns: vec![
                            "name".into(),
                            "key".into(),
                            "role".into(),
                            "id".into(),
                            "created".into(),
                        ],
                        values: result
                            .0
                            .into_iter()
                            .map(|app_key| {
                                serde_json::json! {
                                    [app_key.name, app_key.key, app_key.role,
                                     app_key.identity, app_key.created_date]
                                }
                            })
                            .collect(),
                    }
                    .into())
                };

                futures::pin_mut!(fut);
                ctx.block_on(fut)
            }
        }
    }
}
